//! Perforce (p4.exe) wrapper.
//!
//! v0.5 bootstrap chain:
//!  1. Reach the server (`p4 info`)
//!  2. Write the ticket into %USERPROFILE%\p4tickets.txt so Unreal finds it
//!  3. Create the workspace from the invite's template (Unreal-friendly
//!     options: revertunchanged, LineEnd local)
//!  4. Run the initial `p4 sync //...`
//!
//! v0.5.1 daily ops (post-bootstrap):
//!  - list_changes / change_counts (Changes view + All Projects hints)
//!  - submit_changes (selective Reconcile + Submit)
//!  - restore_file (per-file revert to server's current version)
//!  - force_resync (p4 sync -f)
//!  - reveal_in_explorer (open Explorer at a file)
//!
//! Conflict resolution and File History are queued for v0.6 (see BACKLOG.md).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};
use tokio::process::Command;
use tokio::time::timeout;

use super::errors::CompanionError;
use super::invite::WorkspaceTemplate;
use super::paths;

/// Global serialization lock for `write_ticket_file`. Two concurrent
/// `apply_invite` calls (e.g. the contractor double-clicks Connect, or
/// a deep-link arrives while one is already running) would both read
/// p4tickets.txt, both mutate in memory, and both write back. Last
/// write wins, dropping the other's entry — silently breaking a
/// project that looked like it had onboarded successfully.
///
/// A plain std::sync::Mutex<()> over the entire read-modify-write is
/// sufficient: writes are short, single-process (we already enforce
/// single-instance via tauri-plugin-single-instance), and Windows
/// p4tickets.txt isn't accessed by anyone else mid-Companion-session.
pub(super) static TICKET_FILE_LOCK: Mutex<()> = Mutex::new(());

const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Wall-clock cap for the `p4 reconcile -n //...` step inside adopt_in_place.
/// Reconcile -n walks every tracked file in the workspace and stats it
/// against depot HEAD — on a real-world Unreal project (48 GB / 8000+
/// files measured on VRAYLAR_Neymarc) this can take 1-3 minutes on
/// warm disk. The 30-second CALL_TIMEOUT default is fine for sync -k
/// (pure metadata) but too tight for reconcile -n. 10 minutes is
/// generous enough that a real submission delay surfaces as a clean
/// p4 error rather than a Companion-side timeout that looks broken
/// for no obvious reason.
const ADOPT_RECONCILE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Wall-clock cap for non-streaming long ops like `p4 submit`. Submit
/// uploads file bytes silently with no per-file stdout, so we can't
/// use a streaming idle signal here — a wall-clock cap is the only
/// option. Set generously so realistic submit sizes don't hit it.
const SUBMIT_TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);

/// Maximum wait after EOF for the p4 process to actually exit. Should
/// be near-instant — this is just a safety net so we don't hang forever
/// on a zombie child after stdout closes.
const SYNC_POST_EOF_TIMEOUT: Duration = Duration::from_secs(30);

// NOTE on `p4 sync` timeouts:
//
// We deliberately do NOT impose any timeout on the streaming sync
// (`run_streaming_sync`). p4 emits ONE stdout line per file, AFTER
// the file finishes downloading. A single large Unreal asset (10s
// of GB) can take 30+ minutes on a slow connection, during which p4
// prints nothing — a naive idle timeout would kill that sync even
// though it's working correctly, and retrying would start the same
// big file over from zero, never progressing.
//
// Real stall detection would require monitoring process I/O counters
// (bytes-per-second on disk/network) rather than stdout lines.
// That's planned for a future version with a proper Cancel button;
// see BACKLOG. For now, sync runs to completion or until the user
// quits Companion.
//
// NOTE on byte-level progress (v0.6 investigation — DO NOT re-add `-I -q`):
//
// `p4 -I sync -q` is advertised in Perforce docs as a "progress
// indicator," and it sounds like the fix for the 50 GB single-file
// silent gap above. It is NOT. We tested it against a real depot on
// 2026-05-22 and cross-checked the upstream p4 mailing list
// (perforce-user, "[p4] Useless 'Get Revision Progress' bar"):
//
//   - `-I` reports progress at FILE-count granularity, not bytes.
//     A 1-of-1 sync of a single 50 GB file shows "sync 0% |" then
//     "sync 100% |finishing" with nothing in between — the user
//     still sees no feedback during the long byte transfer.
//
//   - The output format is a terminal-style progress bar with
//     `\b` (0x08) backspace overwrites on a single line, NOT
//     line-delimited records. Captured bytes for a small sync:
//        73 79 6e 63 20 31 30 30 25 20 7c 08 66 69 6e 69 73 68 69 6e 67
//        "sync 100% |\bfinishing\r\n"
//     Not machine-parseable without rewriting our reader to handle
//     terminal control sequences.
//
//   - `-I` is mutually exclusive with `-s`, `-e`, `-G`, and adding it
//     SUPPRESSES the per-file completion lines we currently emit
//     to the UI. Net effect would be strictly less feedback, not
//     more — the user would lose "file N of M synced" and gain
//     nothing in return.
//
// The `ClientProgress` C++ API (p4api) DOES expose per-file byte
// updates via `Update()` callbacks, but the `p4` command-line client
// only surfaces those as the file-count percent bar above. Getting
// real byte-level progress would require linking p4api directly
// (large native dep) or shelling out to p4python. Both are v0.7+
// scope; see BACKLOG.md "byte-level progress for single large files".
//
// What we ship instead today: the file-completion lines from plain
// `p4 sync` stdout, surfaced as `pull-progress` events with `line`
// and a running `pullCount` in the UI. Good feedback for the common
// case (many medium files), insufficient for the worst case (one
// huge file). Worth fixing properly in v0.7, not papering over with
// `-I` in v0.6.

fn no_window() -> u32 {
    0x08000000
}

/// Guard for p4 commands that require the workspace's local root to
/// exist on disk. p4 will happily run with a non-existent --current-dir
/// and produce confusing errors ("Path 'foo' is not under client's
/// root") — fail early with a friendly message instead.
///
/// `change_counts` returns folder_missing=true and recovers gracefully,
/// but every other entry point (list_changes, pull_latest, force_resync,
/// submit_changes) should refuse to run when the folder is gone — the
/// contractor has either deleted or moved it, and they need to pick a
/// new location via the Relocate flow before retrying.
fn verify_workspace_root_exists(root: &Path) -> Result<(), CompanionError> {
    if !root.exists() {
        return Err(CompanionError::Other(format!(
            "Project folder is gone: {}. The folder was deleted or moved — \
             use Relocate to pick a new location, or remove the project and \
             re-onboard.",
            root.display()
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub server_version: String,
    pub server_address: String,
    pub server_uptime: Option<String>,
    pub case_handling: Option<String>,
    pub unicode: Option<String>,
}

#[derive(Clone)]
pub struct P4Config {
    pub server: String,
    pub user: String,
    pub ticket: String,
    pub p4_exe: PathBuf,
}

impl P4Config {
    pub fn cmd(&self) -> Command {
        let mut c = Command::new(&self.p4_exe);
        c.args(["-p", &self.server, "-u", &self.user, "-P", &self.ticket])
            .env("P4CHARSET", "utf8")
            .creation_flags(no_window());
        c
    }
}

async fn run_p4(
    config: &P4Config,
    args: &[&str],
    stdin_text: Option<&str>,
    cwd: Option<&Path>,
    call_timeout: Duration,
) -> Result<(String, String, i32), CompanionError> {
    let mut cmd = config.cmd();
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| CompanionError::Perforce(format!("could not spawn p4: {e}")))?;

    if let Some(text) = stdin_text {
        use tokio::io::AsyncWriteExt;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(text.as_bytes())
                .await
                .map_err(|e| CompanionError::Perforce(format!("stdin write failed: {e}")))?;
            stdin.shutdown().await.ok();
        }
    }

    let output = timeout(call_timeout, child.wait_with_output())
        .await
        .map_err(|_| CompanionError::Perforce(format!("p4 {} timed out", args.join(" "))))?
        .map_err(|e| CompanionError::Perforce(format!("p4 wait failed: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);
    Ok((stdout, stderr, code))
}

/// `p4 info` — also confirms reachability + that the ticket is valid.
pub async fn info(config: &P4Config) -> Result<ServerInfo, CompanionError> {
    let (stdout, stderr, code) = run_p4(config, &["info"], None, None, CALL_TIMEOUT).await?;
    if code != 0 {
        return Err(CompanionError::Perforce(if !stderr.trim().is_empty() {
            stderr
        } else {
            stdout
        }));
    }

    let mut info = ServerInfo {
        server_version: String::new(),
        server_address: String::new(),
        server_uptime: None,
        case_handling: None,
        unicode: None,
    };
    for line in stdout.lines() {
        if let Some((k, v)) = line.split_once(": ") {
            let v = v.trim().to_string();
            match k.trim() {
                "Server version" => info.server_version = v,
                "Server address" => info.server_address = v,
                "Server uptime" => info.server_uptime = Some(v),
                "Case Handling" => info.case_handling = Some(v),
                "Unicode" => info.unicode = Some(v),
                _ => {}
            }
        }
    }
    Ok(info)
}

/// Write the ticket directly into `%USERPROFILE%\p4tickets.txt`. This is
/// the file Unreal's Perforce provider and any standalone p4.exe read by
/// default — populating it means the contractor doesn't have to type the
/// ticket into Unreal's "Source Control" dialog (they paste server/user/
/// workspace and Unreal finds the ticket transparently).
///
/// File format: one line per server, `<server>=<user>:<ticket>`.
/// If the file exists, we replace any existing line for the same
/// (server, user) pair and append otherwise; other entries are
/// preserved verbatim.
pub fn write_ticket_file(
    server: &str,
    user: &str,
    ticket: &str,
) -> Result<PathBuf, CompanionError> {
    // Serialize the read-modify-write so concurrent apply_invite calls
    // can't lose each other's entries. See TICKET_FILE_LOCK comment.
    // Poisoning recovery: if a prior call panicked inside the critical
    // section, the file may be in a partial state — but that's a
    // separate concern; we still hold the lock and proceed.
    let _guard = TICKET_FILE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let path = paths::p4tickets_path().map_err(CompanionError::Perforce)?;

    // p4 has historically stored entries in TWO formats:
    //   <server>=<user>:<ticket>
    //   <server>=<ticket>            (very old)
    // We write only the modern form.
    let new_line = format!("{server}={user}:{ticket}");

    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut kept: Vec<String> = Vec::new();
    let prefix_match = format!("{server}=");
    let user_prefix = format!("{server}={user}:");

    for line in existing.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        // Drop any prior entry for this (server, user) — keep entries for
        // other users on the same server (contractor may also have a
        // personal account on a different tailnet).
        if trimmed.starts_with(&user_prefix) {
            continue;
        }
        // Drop legacy bare-ticket entries for this server, they'd shadow ours.
        if trimmed.starts_with(&prefix_match) && !trimmed.contains(':') {
            continue;
        }
        kept.push(trimmed.to_string());
    }
    kept.push(new_line);

    // Perforce historically writes p4tickets.txt with the read-only
    // attribute set (defense against accidental edits). std::fs::write on
    // Windows refuses to overwrite a read-only file. Clear the bit first
    // if present; we then write and intentionally leave the file
    // writable so future Companion updates work without this dance.
    if path.exists() {
        if let Ok(meta) = std::fs::metadata(&path) {
            let mut perms = meta.permissions();
            if perms.readonly() {
                perms.set_readonly(false);
                if let Err(e) = std::fs::set_permissions(&path, perms) {
                    return Err(CompanionError::Perforce(format!(
                        "could not clear read-only on {}: {e}",
                        path.display()
                    )));
                }
            }
        }
    }

    let body = kept.join("\n") + "\n";
    std::fs::write(&path, body).map_err(|e| {
        CompanionError::Perforce(format!("could not write {}: {e}", path.display()))
    })?;
    Ok(path)
}

/// Create the workspace described in the invite, but with Unreal-friendly
/// SubmitOptions (`revertunchanged`) and LineEnd (`local`) overrides.
/// The invite's `options` line is used verbatim — that's `noallwrite
/// noclobber nocompress unlocked nomodtime normdir` for v=1 contractor
/// invites.
///
/// **Stream binding (added v0.5.3):** If the view's first depot mapping
/// looks like `//<name>/<stream>/...`, the workspace is bound to the
/// stream `//<name>/<stream>`. Spaceshop standardized on stream depots
/// (per the Session 51 foundation handoff), and submits against a
/// stream depot REQUIRE a stream-bound client — otherwise p4 returns
/// "cannot submit from non-stream client" and the user is stuck with
/// pending changes they can't commit. Falls back to classic (no Stream
/// field) if the view doesn't look stream-shaped.
pub async fn create_workspace(
    config: &P4Config,
    template: &WorkspaceTemplate,
    root: &Path,
) -> Result<(), CompanionError> {
    std::fs::create_dir_all(root).map_err(|e| {
        CompanionError::Perforce(format!(
            "could not create workspace folder {}: {e}",
            root.display()
        ))
    })?;

    let view_block: String = template
        .view
        .iter()
        .map(|line| format!("\t{}\n", line))
        .collect();

    // Detect stream from the first view line's depot path.
    let stream_line = template
        .view
        .first()
        .and_then(|line| extract_stream_from_view_line(line))
        .map(|s| format!("Stream:\t{s}\n"))
        .unwrap_or_default();

    let spec = format!(
        "Client:\t{name}\n\
         Owner:\t{owner}\n\
         Host:\t\n\
         Description:\n\tCreated by Spaceshop Companion.\n\
         Root:\t{root}\n\
         Options:\t{options}\n\
         SubmitOptions:\trevertunchanged\n\
         LineEnd:\tlocal\n\
         {stream}\
         View:\n{view}",
        name = template.name,
        owner = config.user,
        root = root.display(),
        options = template.options,
        stream = stream_line,
        view = view_block,
    );

    let (stdout, stderr, code) = run_p4(
        config,
        &["client", "-i"],
        Some(&spec),
        None,
        CALL_TIMEOUT,
    )
    .await?;

    if code != 0 {
        return Err(CompanionError::Perforce(if !stderr.trim().is_empty() {
            stderr
        } else {
            stdout
        }));
    }
    Ok(())
}

/// Auto-heal a workspace that was created before stream-binding support
/// landed. Reads the live spec, and if it lacks a `Stream:` field but
/// the view looks stream-shaped, inserts one and writes the spec back.
/// No-op if the workspace is already stream-bound or the view isn't
/// stream-shaped (classic depot mappings stay classic).
///
/// Used by `force_resync` and `apply_invite` so the "Force re-download"
/// button on an old workspace fixes Submit semantics for free, without
/// the user knowing they had a half-broken workspace.
pub async fn ensure_workspace_stream_bound(
    config: &P4Config,
    workspace: &str,
) -> Result<(), CompanionError> {
    let (spec, stderr, code) = run_p4(
        config,
        &["client", "-o", workspace],
        None,
        None,
        CALL_TIMEOUT,
    )
    .await?;
    if code != 0 {
        return Err(CompanionError::Perforce(if !stderr.trim().is_empty() {
            stderr
        } else {
            spec
        }));
    }

    // Already stream-bound? Nothing to do.
    if spec
        .lines()
        .any(|l| l.trim_start().starts_with("Stream:"))
    {
        return Ok(());
    }

    // Walk into the View block and grab the first view line.
    let mut in_view = false;
    let mut first_view_line: Option<String> = None;
    for line in spec.lines() {
        if line.starts_with("View:") {
            in_view = true;
            continue;
        }
        if in_view {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // First real view line found.
            first_view_line = Some(trimmed.to_string());
            break;
        }
    }
    let stream = match first_view_line.as_deref().and_then(extract_stream_from_view_line) {
        Some(s) => s,
        None => return Ok(()), // classic depot view — leave alone
    };

    // Rewrite the spec with a new Stream: line right before View:.
    let mut new_spec = String::new();
    for line in spec.lines() {
        if line.starts_with("View:") {
            new_spec.push_str(&format!("Stream:\t{}\n", stream));
        }
        new_spec.push_str(line);
        new_spec.push('\n');
    }

    let (_, stderr, code) = run_p4(
        config,
        &["client", "-i"],
        Some(&new_spec),
        None,
        CALL_TIMEOUT,
    )
    .await?;
    if code != 0 {
        return Err(CompanionError::Perforce(format!(
            "could not add Stream binding to workspace: {}",
            stderr.trim()
        )));
    }
    Ok(())
}

/// Parse `Stream: //depot/streamname` out of a workspace-view line like
/// `//smoke-v4/main/... //sarah-test-session-2-smoke/...`. Returns
/// `Some("//smoke-v4/main")` for that input, `None` if the line doesn't
/// look stream-shaped (classic depot mapping, exotic exclusion, etc.).
fn extract_stream_from_view_line(line: &str) -> Option<String> {
    // First whitespace-separated token is the depot side of the mapping.
    let depot_side = line.trim().split_whitespace().next()?;
    let depot_side = depot_side.trim_start_matches(['-', '+']); // exclude/overlay markers
    if !depot_side.starts_with("//") {
        return None;
    }
    // Strip a trailing /... or /...* — what's left is //depot/stream.
    // Stream paths are exactly two segments past //: //<depot>/<stream>.
    let stripped = depot_side
        .trim_end_matches("/...")
        .trim_end_matches("/...*");
    let segments: Vec<&str> = stripped.trim_start_matches("//").split('/').collect();
    if segments.len() < 2 {
        return None;
    }
    // First two non-empty segments form the stream root.
    let depot = segments[0];
    let stream = segments[1];
    if depot.is_empty() || stream.is_empty() {
        return None;
    }
    Some(format!("//{depot}/{stream}"))
}

/// Initial onboarding sync. Prepends the same revert+flush sequence as
/// `force_resync` so re-onboarding into an existing workspace heals
/// pending stuck-opens (e.g., files left "open for delete" from a
/// previous failed Submit) before running `p4 sync //...`. Without this,
/// a re-onboard can leave the folder empty and the user unable to
/// recover without CLI surgery — bug class we hit on 2026-05-22.
pub async fn initial_sync<F>(
    config: &P4Config,
    workspace: &str,
    workspace_root: &Path,
    on_progress: F,
) -> Result<u32, CompanionError>
where
    F: FnMut(&str),
{
    // Reuse force_resync's revert+flush+sync logic — onboarding has the
    // same "trust the server, nuke local state" semantics. Stream binding
    // gets auto-healed too as a side effect.
    force_resync(config, workspace, workspace_root, on_progress).await
}

/// Outcome summary of an adopt-in-place onboarding. Returned to the
/// caller so the UI can surface a meaningful "you have N pending
/// changes" message when local state diverges from depot.
#[derive(Debug, Clone, Default)]
pub struct AdoptOutcome {
    /// Files counted in the have-table after `p4 sync -k`. Equals the
    /// total file count of the workspace at the pinned revision —
    /// useful for the persisted project record's `last_sync_files`.
    pub have_table_files: u32,

    /// Number of files where local disk content differs from depot
    /// (modified, missing, or extra). Zero means perfect adoption.
    /// Non-zero is NOT an error — it represents legitimate pending
    /// work the operator did between MIGRATE and self-onboard, or
    /// stale workspace content if adopt_at_cl < depot HEAD.
    pub reconcile_diff_count: u32,
}

/// Adopt-in-place onboarding for workspaces where the files are
/// ALREADY ON DISK at `workspace_root` (typical: arsen self-onboarding
/// a project Workshop just migrated). Replaces force_resync's
/// destructive flush+sync with a have-table-only update + reconcile
/// audit:
///
///   1. `p4 sync -k //...[@N]` — tells the server "I have all these
///      files at revision N (or HEAD if `at_cl=None`)." Pure metadata
///      update, NO file transfer. Completes in seconds even for 50GB
///      workspaces.
///
///   2. `p4 reconcile -n //...` — preview-only audit of what would be
///      opened-for-add/edit/delete if the operator submitted now.
///      Returns the count so the UI can decide whether to surface a
///      "X pending changes detected during adoption" notice.
///
/// Pre-flight: verifies the workspace root exists AND is non-empty.
/// An empty root means the adopt-existing claim is unsupported by
/// reality — caller should fall back to initial_sync.
///
/// Streams no progress (both ops are fast metadata calls), but emits
/// a single "adoption complete" progress line through `on_progress`
/// so the frontend's step-status UI has something to display.
pub async fn adopt_in_place<F>(
    config: &P4Config,
    workspace: &str,
    workspace_root: &Path,
    at_cl: Option<u32>,
    mut on_progress: F,
) -> Result<AdoptOutcome, CompanionError>
where
    F: FnMut(&str),
{
    verify_workspace_root_exists(workspace_root)?;

    // Defensive — if the root is empty, adoption is a lie. Caller (in
    // onboarding.rs) is expected to short-circuit to initial_sync
    // BEFORE getting here, but double-check so a malformed self-onboard
    // invite doesn't silently produce a workspace claiming "all 8000
    // files at HEAD" when the disk is empty.
    let has_content = std::fs::read_dir(workspace_root)
        .map_err(|e| {
            CompanionError::Perforce(format!(
                "could not read workspace root '{}': {e}",
                workspace_root.display()
            ))
        })?
        .next()
        .is_some();
    if !has_content {
        return Err(CompanionError::Perforce(format!(
            "adopt-in-place was invoked but workspace root '{}' is empty. \
             Either the invite is mis-targeted, or fall back to initial_sync.",
            workspace_root.display()
        )));
    }

    // Heal workspace stream binding (mirrors force_resync — newer Workshop
    // builds always bind, older specs in the wild may not).
    let _ = ensure_workspace_stream_bound(config, workspace).await;

    // ----- Step 1: p4 sync -k //...[@N] -----
    //
    // -k = update have-table only, NO file transfer.
    // @N = pin to a specific changelist if provided; default is HEAD.
    //
    // Per-file stdout lines look like:
    //   //depot/path/file.uasset#1 - updating //ws/path/file.uasset
    // We count these as `have_table_files`.
    on_progress("Updating have-table from local disk state…");
    let sync_target = match at_cl {
        Some(n) => format!("//...@{n}"),
        None => "//...".to_string(),
    };
    let (stdout, stderr, code) = run_p4(
        config,
        &["-c", workspace, "sync", "-k", &sync_target],
        None,
        Some(workspace_root),
        CALL_TIMEOUT,
    )
    .await?;
    // `p4 sync -k` exit 1 + stderr "file(s) up-to-date" is the benign
    // "nothing changed" case (the workspace's have-table was already
    // at this revision — re-running adoption). Any other non-zero
    // exit is a real failure.
    if code != 0 && !stderr.contains("up-to-date") {
        return Err(CompanionError::Perforce(format!(
            "p4 sync -k failed (exit {code}): {}",
            stderr.trim()
        )));
    }
    // `p4 sync` per-file verbs we count toward have_table_files:
    //   - updating   : moving the recorded revision forward
    //   - added as   : first-time entry in the have-table (typical
    //                  for fresh adoption from empty have-table)
    //   - refreshing : have-table already at this rev; -k re-stamps
    //                  it (re-running adoption produces these)
    //   - deleted as : file was deleted in the depot at the pinned
    //                  CL; have-table is updated to "not present"
    // Without the latter two we'd see "Adopted 0 files" on a
    // re-run, even though every file is correctly accounted for.
    let have_table_files: u32 = stdout
        .lines()
        .filter(|line| {
            line.contains(" - updating ")
                || line.contains(" - added as ")
                || line.contains(" - refreshing ")
                || line.contains(" - deleted as ")
        })
        .count() as u32;

    // ----- Step 2: p4 reconcile -n //... -----
    //
    // -n = preview only, do NOT open files. Lists files where disk
    // content diverges from depot HEAD. Three flavors:
    //
    //   //depot/path/x#1 - reconcile (add/edit/delete) //ws/path/x
    //
    // We just count divergences; the frontend decides whether to
    // surface a "your local copy differs from depot" notice.
    on_progress("Auditing local disk vs depot HEAD…");
    let (rstdout, rstderr, rcode) = run_p4(
        config,
        &["-c", workspace, "reconcile", "-n", "//..."],
        None,
        Some(workspace_root),
        ADOPT_RECONCILE_TIMEOUT,
    )
    .await?;
    // Reconcile exit 1 + stderr "no file(s) to reconcile" is the
    // happy path (perfect adoption). Anything else non-zero is real.
    if rcode != 0 && !rstderr.contains("no file(s) to reconcile") {
        return Err(CompanionError::Perforce(format!(
            "p4 reconcile -n failed (exit {rcode}): {}",
            rstderr.trim()
        )));
    }
    let reconcile_diff_count: u32 = rstdout
        .lines()
        .filter(|line| line.contains(" - reconcile "))
        .count() as u32;

    on_progress(&format!(
        "Adoption complete — {have_table_files} files registered, \
         {reconcile_diff_count} divergences from depot."
    ));

    Ok(AdoptOutcome {
        have_table_files,
        reconcile_diff_count,
    })
}

/// `p4 sync //...` — daily Pull Latest. Unlike `initial_sync` this does
/// NOT pass `-f`, so files with pending local edits are left alone
/// (Perforce will yell about them via stderr) and unchanged files are
/// not re-downloaded. Use this for the "I want the server's latest
/// changes" path; use `initial_sync` for onboarding/relocate where
/// repopulating an empty folder is the goal.
///
/// Uses per-line idle timeout — sync can run indefinitely as long as
/// new file lines keep arriving, but fails fast if the stream stalls.
pub async fn sync_workspace<F>(
    config: &P4Config,
    workspace: &str,
    workspace_root: &Path,
    on_progress: F,
) -> Result<u32, CompanionError>
where
    F: FnMut(&str),
{
    verify_workspace_root_exists(workspace_root)?;
    run_streaming_sync(
        config,
        workspace,
        workspace_root,
        &["sync", "//..."],
        on_progress,
        "pull",
    )
    .await
}

// ============================================================================
// Daily-use ops added in v0.5.1: list_changes, submit, restore, force_resync,
// change_counts, conflict detection + resolution.
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedFile {
    /// Status letter — "M" (modified), "A" (added/new), "D" (deleted).
    pub status: String,
    /// Absolute local-disk path.
    pub local_path: String,
    /// Depot path the file maps to.
    pub depot_path: String,
    /// File size in bytes if known. None for deleted files.
    pub size: Option<u64>,
}

/// `p4 status` — flat list of files that differ from the depot (modified,
/// added, deleted). Two distinct output formats depending on whether
/// the file is already opened in a pending changelist or not:
///
///   <local-relative-path> - reconcile to <action> <depot-path>#<rev>
///       — file is NOT yet opened; needs `p4 reconcile` to track it
///
///   <local-relative-path> - submit change <N> to <action> <depot-path>#<rev>
///       — file is already in a pending changelist (e.g., left over
///         from a prior failed submit); ready to ship as-is
///
/// We treat both the same way for the Changes view — they're both
/// "files that differ from the server" — but the parser MUST recognize
/// both or it silently skips already-opened files. Skipping them was
/// the cause of the "I had 3 changes but only saw 2" bug on
/// 2026-05-22.
///
/// p4 status may also emit "warning" continuation lines that start with
/// `... //depot/...` — those are diagnostic for the previous file and
/// don't contain either of the two split tokens; we skip those.
///
/// Local path is RELATIVE to the workspace root; we join with the root
/// to produce an absolute path for the UI (Reveal-in-Explorer needs it).
/// List the files this workspace has opened for action (edit/add/delete)
/// in any changelist (default or numbered).
///
/// **v0.6.4 rewrite — `p4 opened` instead of `p4 status`.**
///
/// Pre-v0.6.4, `list_changes` called `p4 -c <ws> status` which walks
/// every file in the workspace comparing disk vs depot. On a real-world
/// Unreal project (48 GB / 8000+ files, measured on VRAYLAR_Neymarc)
/// that call takes 52-66 seconds, well over the 30-second
/// `CALL_TIMEOUT`. Result: the "Review & Submit" page in the main
/// window would either time out (showing "no local changes" — wrong)
/// or hang the UI mid-spinner. Operator's reproducible complaint:
/// "i had one file marked for add, and one checked out, yet when i
/// go to companion the review & submit is grayed out and it says no
/// local changes."
///
/// **The fix.** Use `p4 -ztag -c <ws> opened` instead. `p4 opened`
/// reads from p4d's `db.working` table for this client only — purely
/// server-side metadata, sub-100 ms. Returns every file the workspace
/// has open for edit/add/delete in ANY changelist, with both the
/// depotFile and clientFile paths in tagged form for clean parsing.
///
/// **What this loses.** `p4 opened` does NOT catch unmanaged disk
/// changes — files that were modified or created on disk without
/// being opened via `p4 edit` / `p4 add`. Unreal's source-control
/// plugin auto-runs `p4 edit` on checkout and `p4 add` on new assets,
/// so for the standard UE workflow we don't lose anything. Manual
/// notepad edits to `Config/Default*.ini` or similar would be
/// invisible until the user runs Reconcile explicitly (a future
/// "Scan for unmanaged files" button is the right home for that —
/// not the every-time path).
pub async fn list_changes(
    config: &P4Config,
    workspace: &str,
    workspace_root: &Path,
) -> Result<Vec<ChangedFile>, CompanionError> {
    verify_workspace_root_exists(workspace_root)?;

    // `-ztag` produces one tagged record per opened file:
    //   ... depotFile //depot/path/file.uasset
    //   ... clientFile //workspace-name/path/file.uasset
    //   ... rev 1
    //   ... haveRev none|N
    //   ... action add|edit|delete|...
    //   ... change default|N
    //   ... type binary+l|binary|text|...
    //   ... user arsen
    //   ... client arsen-vraylar-neymarcbrothers
    //   (blank line separator)
    let (stdout, stderr, code) = run_p4(
        config,
        &["-ztag", "-c", workspace, "opened"],
        None,
        Some(workspace_root),
        CALL_TIMEOUT,
    )
    .await?;

    // "File(s) not opened on this client." is exit-1 + that message
    // in stderr — the benign "nothing open" case, NOT an error.
    if code != 0 {
        let stderr_lc = stderr.to_lowercase();
        if stderr_lc.contains("not opened on this client") {
            return Ok(Vec::new());
        }
        return Err(CompanionError::Perforce(if !stderr.trim().is_empty() {
            stderr
        } else {
            stdout
        }));
    }

    // Workspace name as it appears in `clientFile` records — used to
    // strip the leading `//<workspace>/` and rejoin against the
    // local workspace root for the `local_path` field consumers expect.
    let client_prefix = format!("//{workspace}/");

    let mut out: Vec<ChangedFile> = Vec::new();
    let mut cur_depot: Option<String> = None;
    let mut cur_client: Option<String> = None;
    let mut cur_action: Option<String> = None;

    let flush = |depot: &mut Option<String>,
                 client: &mut Option<String>,
                 action: &mut Option<String>,
                 out: &mut Vec<ChangedFile>| {
        if let (Some(d), Some(c), Some(a)) =
            (depot.take(), client.take(), action.take())
        {
            // action → ChangedFile status code that the frontend renders
            //   edit       → "M" (modified)
            //   add / branch → "A" (added)
            //   delete / move/delete → "D" (deleted)
            // Anything we don't recognize is dropped rather than mis-
            // labeled — better to under-report than to surface a "?".
            let status = match a.as_str() {
                "edit" | "integrate" => "M",
                "add" | "branch" | "move/add" => "A",
                "delete" | "move/delete" => "D",
                _ => return,
            };

            // Strip the `//<workspace>/` prefix from clientFile and
            // rejoin against the local workspace_root. p4 produces a
            // forward-slash path; PathBuf::join handles separator
            // conversion on Windows.
            let rel = if let Some(rest) = c.strip_prefix(&client_prefix) {
                rest.to_string()
            } else {
                // clientFile didn't start with our workspace prefix —
                // could be from an alternate AltRoots binding. Use the
                // whole clientFile as the local path lossily.
                c.clone()
            };
            let local_path = workspace_root.join(&rel).to_string_lossy().into_owned();
            let size = std::fs::metadata(&local_path).ok().map(|m| m.len());
            out.push(ChangedFile {
                status: status.into(),
                local_path,
                depot_path: d,
                size,
            });
        }
    };

    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // Blank line marks the end of one record. Flush whatever
            // we've accumulated.
            flush(&mut cur_depot, &mut cur_client, &mut cur_action, &mut out);
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("... ") else {
            // Unexpected non-tag line — skip without touching state.
            continue;
        };
        let Some((key, val)) = rest.split_once(' ') else {
            continue;
        };
        match key {
            "depotFile" => cur_depot = Some(val.to_string()),
            "clientFile" => cur_client = Some(val.to_string()),
            "action" => cur_action = Some(val.to_string()),
            _ => {}
        }
    }
    // Trailing record (no blank line at EOF) — flush.
    flush(&mut cur_depot, &mut cur_client, &mut cur_action, &mut out);

    Ok(out)
}

/// Quick counts for the All Projects status hints:
///  - local_pending: number of files that differ from the depot locally
///  - remote_unseen: number of files that would change if we pulled now
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeCounts {
    pub local_pending: u32,
    pub remote_unseen: u32,
    /// True if the project's workspace_root directory no longer exists
    /// on disk (user deleted/moved it). When set, the other counts are
    /// not meaningful — UI should prompt the contractor to pick a new
    /// folder and re-sync.
    pub folder_missing: bool,
}

/// Cheap-query change-counts (v0.6.3 rewrite).
///
/// **The bug this replaces.** Pre-v0.6.3 `change_counts` ran:
///   1. `p4 -c <ws> status`        — walks every file in the workspace,
///                                    compares disk vs depot. Measured
///                                    52 s — 4 min on real Unreal projects
///                                    (8000+ files, 48 GB).
///   2. `p4 -c <ws> sync -n //...` — another full workspace walk.
///
/// Each call took a server-side read lock on the workspace's db.locks
/// row. With the tray polling every 30 s, multiple in-flight statuses
/// would stack and starve every other p4 op against the same workspace
/// — Unreal's `p4 edit` for a single asset blocked for 90+ s under
/// the lock storm. (See Session 5 SESSIONS entry.)
///
/// **The fix.** Replace both with metadata-only queries:
///
///   - `p4 -c <ws> opened` — lists files the workspace has open for
///     edit/add/delete. The count IS `local_pending`. Sub-100 ms
///     even on huge workspaces because p4d just reads the
///     db.working table for this client. No file-content I/O.
///
///   - `p4 changes -m 1 //<view>/...` — gets the most recent depot CL
///     visible to this workspace. We compare against our persisted
///     `last_sync_at_cl` (when present) — if depot HEAD > last sync,
///     `remote_unseen` is `1` (boolean-enough for the tray's green/
///     yellow distinction). The precise "files pulled if you sync"
///     number is no longer surfaced for the tray; if a project wants
///     the precise count it can call `list_changes` on demand.
///
/// Both are pure server-side metadata queries that complete in tens
/// of milliseconds. The tray poll cycle drops from minutes to
/// sub-second, eliminating lock contention with Unreal entirely.
pub async fn change_counts(
    config: &P4Config,
    workspace: &str,
    workspace_root: &Path,
) -> Result<ChangeCounts, CompanionError> {
    if !workspace_root.exists() {
        return Ok(ChangeCounts {
            local_pending: 0,
            remote_unseen: 0,
            folder_missing: true,
        });
    }

    // ----- local_pending: files this workspace has open for action -----
    //
    // `p4 opened` exit codes:
    //   0 with one line per opened file
    //   1 with stderr "File(s) not opened on this client." when none open
    //
    // The "none open" case is NOT a real error. Anything else IS.
    let (stdout, stderr, code) = run_p4(
        config,
        &["-c", workspace, "opened"],
        None,
        Some(workspace_root),
        CALL_TIMEOUT,
    )
    .await?;

    let local_pending = if code == 0 {
        stdout
            .lines()
            .filter(|l| {
                let l = l.trim();
                // Each opened-file line starts with `//depot/...`. Skip
                // tagged output or stray blanks.
                !l.is_empty() && l.starts_with("//")
            })
            .count() as u32
    } else if stderr.to_lowercase().contains("not opened on this client") {
        0
    } else {
        return Err(CompanionError::Perforce(if !stderr.trim().is_empty() {
            stderr
        } else {
            stdout
        }));
    };

    // ----- remote_unseen: depot has changed since our last sync? -----
    //
    // `p4 changes -m 1 //<view>/...` returns the most recent depot CL
    // visible through the workspace view (post-typemap, post-stream).
    // We compare against the workspace's "have" max via `p4 changes
    // -m 1 //<view>/...#have` — that gives the highest CL the
    // workspace's have-table represents.
    //
    // If they differ, remote has unseen content. We surface this as
    // a boolean (1 == has unseen, 0 == up-to-date) rather than a
    // precise file count, because the file count would require the
    // expensive `p4 sync -n //...` we're removing.
    //
    // Derive the depot stream path from the workspace name (lowest-
    // friction approach — Companion's projects.json doesn't store
    // the depot path directly, but the workspace IS named
    // <user>-<depot> by convention and the stream is //<depot>/main).
    // If derivation fails, fall back to `//...` which queries the
    // user's entire visible depot space — slightly broader than we'd
    // like for a multi-project user but still cheap.
    let view_path = workspace
        .strip_prefix(&format!("{}-", config.user))
        .map(|depot| format!("//{depot}/..."))
        .unwrap_or_else(|| "//...".to_string());

    let head_cl = depot_head_cl(config, workspace, workspace_root, &view_path).await?;
    let have_cl = depot_have_cl(config, workspace, workspace_root, &view_path).await?;

    let remote_unseen = match (head_cl, have_cl) {
        (Some(head), Some(have)) if head > have => 1,
        (Some(_), None) => 1, // never synced — remote has everything
        _ => 0,
    };

    Ok(ChangeCounts {
        local_pending,
        remote_unseen,
        folder_missing: false,
    })
}

/// Highest CL number visible in the depot through `view_path`.
/// Cheap server-side metadata read — no workspace walk.
async fn depot_head_cl(
    config: &P4Config,
    workspace: &str,
    workspace_root: &Path,
    view_path: &str,
) -> Result<Option<u32>, CompanionError> {
    let (stdout, _stderr, code) = run_p4(
        config,
        &["-c", workspace, "changes", "-m", "1", view_path],
        None,
        Some(workspace_root),
        CALL_TIMEOUT,
    )
    .await?;
    // Empty depot (no submitted CLs) is "exit 0 with empty stdout".
    if code != 0 {
        // Treat any non-zero as "can't determine HEAD" — caller
        // collapses to remote_unseen=0 rather than propagating an
        // error that would mark the project as p4-failed for a
        // benign empty-depot case. Real connection errors surface
        // through `info` / `opened` (run earlier in the cycle).
        return Ok(None);
    }
    Ok(parse_change_number(&stdout))
}

/// Highest CL the workspace's have-table represents through
/// `view_path`. Compares against `depot_head_cl` to detect remote-
/// has-newer.
async fn depot_have_cl(
    config: &P4Config,
    workspace: &str,
    workspace_root: &Path,
    view_path: &str,
) -> Result<Option<u32>, CompanionError> {
    // `#have` is Perforce's revision shorthand for "the revision
    // recorded in MY have-table." `changes -m 1 //view/...#have`
    // returns the highest CL number any file in our have-table
    // points to — i.e., what we last synced.
    let path_with_have = format!("{view_path}#have");
    let (stdout, _stderr, code) = run_p4(
        config,
        &["-c", workspace, "changes", "-m", "1", &path_with_have],
        None,
        Some(workspace_root),
        CALL_TIMEOUT,
    )
    .await?;
    if code != 0 {
        return Ok(None);
    }
    Ok(parse_change_number(&stdout))
}

/// Pull the first `Change NNN` integer out of `p4 changes` output.
fn parse_change_number(stdout: &str) -> Option<u32> {
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Change ") {
            if let Some(end) = rest.find(|c: char| !c.is_ascii_digit()) {
                if let Ok(n) = rest[..end].parse::<u32>() {
                    return Some(n);
                }
            } else if let Ok(n) = rest.parse::<u32>() {
                return Some(n);
            }
        }
    }
    None
}

/// Relocate a project's workspace to a new local folder.
///
/// Steps:
///   1. Ensure the new folder exists
///   2. Read the current `p4 client` spec
///   3. Replace the `Root:` field with the new path, write back via `p4 client -i`
///   4. Update Companion's persisted record
///   5. Run a fresh `p4 sync //...` into the new folder
pub async fn relocate_workspace(
    config: &P4Config,
    workspace: &str,
    new_root: &Path,
) -> Result<(), CompanionError> {
    std::fs::create_dir_all(new_root).map_err(CompanionError::Io)?;

    let (stdout, stderr, code) = run_p4(
        config,
        &["client", "-o", workspace],
        None,
        None,
        CALL_TIMEOUT,
    )
    .await?;
    if code != 0 {
        return Err(CompanionError::Perforce(if !stderr.trim().is_empty() {
            stderr
        } else {
            stdout
        }));
    }

    // Rewrite the Root field. `p4 client -o` emits a complete spec with
    // comment lines starting with '#' — we keep those verbatim. Only the
    // Root line gets replaced.
    let mut new_spec = String::new();
    let mut found_root = false;
    for line in stdout.lines() {
        if !found_root && line.trim_start().starts_with("Root:") {
            new_spec.push_str(&format!("Root:\t{}\n", new_root.display()));
            found_root = true;
        } else {
            new_spec.push_str(line);
            new_spec.push('\n');
        }
    }
    if !found_root {
        // Workspace spec didn't have a Root line — that's broken. Bail.
        return Err(CompanionError::Perforce(
            "workspace spec is missing a Root field".into(),
        ));
    }

    let (_, stderr, code) = run_p4(
        config,
        &["client", "-i"],
        Some(&new_spec),
        None,
        CALL_TIMEOUT,
    )
    .await?;
    if code != 0 {
        return Err(CompanionError::Perforce(stderr.trim().into()));
    }
    Ok(())
}

#[tauri::command]
pub async fn p4_relocate_project<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    project_id: String,
    new_root: String,
) -> Result<u32, CompanionError> {
    use super::projects::{save_persisted, ProjectState};

    let _pulling = PullingGuard::new(app.clone(), project_id.clone());

    // Look up project + cfg
    let (cfg, workspace, _) = {
        let state = app.state::<ProjectState>();
        let projects = state.snapshot();
        let project = projects
            .iter()
            .find(|p| p.project_id == project_id)
            .cloned()
            .ok_or_else(|| {
                CompanionError::Other(format!("no project {project_id}"))
            })?;
        let p4_exe = PathBuf::from(
            paths::p4_exe(&app).map_err(CompanionError::Perforce)?,
        );
        (
            P4Config {
                server: project.server.clone(),
                user: project.user.clone(),
                ticket: project.ticket.clone(),
                p4_exe,
            },
            project.workspace_name.clone(),
            PathBuf::from(project.workspace_root.clone()),
        )
    };

    let new_path = PathBuf::from(&new_root);
    relocate_workspace(&cfg, &workspace, &new_path).await?;

    // Update persisted state with the new root.
    {
        let state = app.state::<ProjectState>();
        let mut projects = state.snapshot();
        for p in projects.iter_mut() {
            if p.project_id == project_id {
                p.workspace_root = new_root.clone();
            }
        }
        state.replace_all(projects.clone());
        save_persisted(&app, &projects).map_err(CompanionError::Other)?;
    }

    // Re-sync into the new folder.
    let app_for_progress = app.clone();
    let pid = project_id.clone();
    initial_sync(&cfg, &workspace, &new_path, move |line| {
        let _ = app_for_progress.emit(
            "pull-progress",
            serde_json::json!({ "project_id": pid, "line": line, "relocating": true, "source": "main" }),
        );
    })
    .await
}

/// `p4 revert -k` everything currently opened in the default changelist —
/// preserves on-disk content, just un-opens. Used before a selective
/// reconcile so that prior runs don't pollute the new submission.
async fn revert_keep_all_opened(
    config: &P4Config,
    workspace: &str,
    workspace_root: &Path,
) -> Result<(), CompanionError> {
    let (_, stderr, code) = run_p4(
        config,
        &["-c", workspace, "revert", "-k", "//..."],
        None,
        Some(workspace_root),
        CALL_TIMEOUT,
    )
    .await?;
    // "file(s) not opened on this client" exit 1 is fine — nothing to revert.
    if code != 0 && !stderr.to_lowercase().contains("not opened on this client") {
        return Err(CompanionError::Perforce(stderr.trim().into()));
    }
    Ok(())
}

/// Submit a selection of files with a description.
///
/// Sequence:
///   1. `p4 revert -k //...` — un-open any files in the default changelist
///      (preserves local content) so we're starting from a clean pending list
///   2. `p4 reconcile <local_paths>` — opens the selected files for edit/add/delete
///   3. `p4 submit -d "<description>"` — submits the default changelist
///
/// Returns the submitted changelist number.
pub async fn submit_changes(
    config: &P4Config,
    workspace: &str,
    workspace_root: &Path,
    selected_local_paths: &[String],
    description: &str,
) -> Result<i32, CompanionError> {
    verify_workspace_root_exists(workspace_root)?;
    if selected_local_paths.is_empty() {
        return Err(CompanionError::Perforce("no files selected".into()));
    }
    if description.trim().is_empty() {
        return Err(CompanionError::Perforce(
            "description is required".into(),
        ));
    }

    // Auto-heal: if the workspace was created before stream-binding
    // support landed, add `Stream:` from the view now so this submit
    // doesn't fail with "cannot submit from non-stream client".
    // Best-effort — if heal fails we still try the submit; the user
    // gets the original p4 error if it doesn't work.
    let _ = ensure_workspace_stream_bound(config, workspace).await;

    revert_keep_all_opened(config, workspace, workspace_root).await?;

    // Reconcile the selected files (opens them for the right action).
    let mut args: Vec<String> = vec![
        "-c".into(),
        workspace.into(),
        "reconcile".into(),
        "-e".into(),
        "-a".into(),
        "-d".into(),
    ];
    args.extend(selected_local_paths.iter().cloned());
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let (_, stderr, code) = run_p4(
        config,
        &arg_refs,
        None,
        Some(workspace_root),
        CALL_TIMEOUT * 2,
    )
    .await?;
    if code != 0 && !stderr.to_lowercase().contains("no file(s) to reconcile") {
        return Err(CompanionError::Perforce(stderr.trim().into()));
    }

    // Submit.
    let (stdout, stderr, code) = run_p4(
        config,
        &["-c", workspace, "submit", "-d", description],
        None,
        Some(workspace_root),
        SUBMIT_TIMEOUT,
    )
    .await?;
    if code != 0 {
        return Err(CompanionError::Perforce(if !stderr.trim().is_empty() {
            stderr
        } else {
            stdout
        }));
    }

    // Parse the submitted CL number — last "Change NNN submitted." line.
    for line in stdout.lines().rev() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Change ") {
            if let Some((num, _)) = rest.split_once(' ') {
                if let Ok(n) = num.parse::<i32>() {
                    return Ok(n);
                }
            }
        }
    }
    Ok(-1)
}

/// Restore a file to its server version — throws away local edits.
///
/// Sequence:
///   1. `p4 reconcile <file>` — open it for whichever action the diff implies
///   2. `p4 revert <file>` — discard the opened change, restoring the
///      server's current content on disk
pub async fn restore_file(
    config: &P4Config,
    workspace: &str,
    workspace_root: &Path,
    local_path: &str,
) -> Result<(), CompanionError> {
    verify_workspace_root_exists(workspace_root)?;
    // Reconcile this file (opens for edit/add/delete depending on its state).
    let (_, _, _) = run_p4(
        config,
        &[
            "-c", workspace, "reconcile", "-e", "-a", "-d", local_path,
        ],
        None,
        Some(workspace_root),
        CALL_TIMEOUT,
    )
    .await?;

    // Revert it.
    let (_, stderr, code) = run_p4(
        config,
        &["-c", workspace, "revert", local_path],
        None,
        Some(workspace_root),
        CALL_TIMEOUT,
    )
    .await?;
    if code != 0 && !stderr.to_lowercase().contains("file(s) not opened") {
        return Err(CompanionError::Perforce(stderr.trim().into()));
    }
    Ok(())
}

/// Reset the workspace back to the server's current state. Three-step
/// sequence because plain `p4 sync -f` isn't enough on its own:
///
///   1. `p4 revert -k //...` — unopens any files in the pending changelist
///      (e.g., files stuck "open for delete" from a prior failed submit).
///      Without this, flush refuses with "is opened for delete and can't
///      be deleted" and the user is permanently stuck.
///   2. `p4 flush //...#0` — clears the have-table so the server forgets
///      we ever synced anything. Required because the contractor's
///      folder may have been deleted manually, leaving p4's have-table
///      pointing at files that don't exist.
///   3. `p4 sync //...` — pulls every file fresh into the workspace root.
///
/// Destructive: clobbers local edits, drops pending changes. Caller MUST
/// present a YES-gate confirm UI first.
pub async fn force_resync<F>(
    config: &P4Config,
    workspace: &str,
    workspace_root: &Path,
    on_progress: F,
) -> Result<u32, CompanionError>
where
    F: FnMut(&str),
{
    verify_workspace_root_exists(workspace_root)?;
    // 0. Heal workspace spec if it pre-dates stream-binding support.
    //    Best-effort; if it fails, sync may still work for read-only ops.
    let _ = ensure_workspace_stream_bound(config, workspace).await;

    // 1. revert -k everything (ignore exit code; "not opened on this client" is fine)
    let _ = run_p4(
        config,
        &["-c", workspace, "revert", "-k", "//..."],
        None,
        Some(workspace_root),
        CALL_TIMEOUT,
    )
    .await;

    // 2. flush the have-table back to "I have nothing"
    let _ = run_p4(
        config,
        &["-vnet.maxwait=300", "-c", workspace, "flush", "//...#0"],
        None,
        Some(workspace_root),
        CALL_TIMEOUT,
    )
    .await;

    // 3. plain sync (not -f) — flush already reset have-table so sync
    //    will pull everything fresh
    force_resync_inner(config, workspace, workspace_root, on_progress).await
}

async fn force_resync_inner<F>(
    config: &P4Config,
    workspace: &str,
    workspace_root: &Path,
    on_progress: F,
) -> Result<u32, CompanionError>
where
    F: FnMut(&str),
{
    run_streaming_sync(
        config,
        workspace,
        workspace_root,
        &["sync", "//..."],
        on_progress,
        "force resync",
    )
    .await
}

/// Surgical recovery for a workspace that drifted out of sync with the
/// server but where Pull Latest isn't fixing it (orphan files on disk
/// that the server doesn't know about, partial-sync interruption, a
/// corrupted have-table, etc.).
///
/// Sequence:
///   1. `p4 clean //...` — removes orphan files (anything on disk the
///      server doesn't track) and reverts files that were edited
///      WITHOUT being opened for edit. Critically, this does NOT touch
///      files that are properly opened in a pending changelist — so
///      Repair is safe to run with pending work in flight, unlike
///      Force re-download which nukes everything.
///   2. `p4 sync -f //...` — re-pulls every file from the server,
///      ignoring the have-table. Heals the case where the have-table
///      claims we have files that aren't actually on disk (or have the
///      wrong content), which plain `p4 sync` won't fix.
///
/// Streams progress identically to `force_resync` — same `pull-progress`
/// event consumers on the frontend work without changes.
///
/// Pre-flight: verifies the workspace root exists; bails with the
/// standard "folder is gone, use Relocate" error otherwise.
pub async fn repair_workspace<F>(
    config: &P4Config,
    workspace: &str,
    workspace_root: &Path,
    mut on_progress: F,
) -> Result<u32, CompanionError>
where
    F: FnMut(&str),
{
    verify_workspace_root_exists(workspace_root)?;

    // Best-effort stream binding heal — keeps repair useful on old
    // pre-stream-bound workspaces, same pattern as force_resync.
    let _ = ensure_workspace_stream_bound(config, workspace).await;

    // 1. p4 clean //... — remove orphans, restore unopened edits.
    //    Streams one line per affected file the same way sync does,
    //    so we reuse run_streaming_sync (the helper is misnamed but
    //    handles any streaming p4 op).
    run_streaming_sync(
        config,
        workspace,
        workspace_root,
        &["clean", "//..."],
        &mut on_progress,
        "repair clean",
    )
    .await?;

    // 2. p4 sync -f //... — force re-pull from server, ignoring have-table.
    run_streaming_sync(
        config,
        workspace,
        workspace_root,
        &["sync", "-f", "//..."],
        on_progress,
        "repair sync",
    )
    .await
}

/// Shared implementation for streaming `p4 sync` variants. Spawns the
/// child, streams stdout line-by-line emitting each one as progress,
/// and uses an idle timeout (no progress in SYNC_IDLE_TIMEOUT) instead
/// of a wall-clock cap on total duration. Real Unreal projects can
/// legitimately take many hours to pull on slow connections — that is
/// not an error, as long as files are still flowing.
async fn run_streaming_sync<F>(
    config: &P4Config,
    workspace: &str,
    workspace_root: &Path,
    p4_args: &[&str],
    mut on_progress: F,
    label: &str,
) -> Result<u32, CompanionError>
where
    F: FnMut(&str),
{
    use tokio::io::{AsyncBufReadExt, BufReader};

    let mut cmd = Command::new(&config.p4_exe);
    let base = [
        "-p", &config.server, "-u", &config.user, "-P", &config.ticket,
        "-c", workspace,
        // -vnet.maxwait=300: p4-native socket idle timeout (5 min).
        // Errors out only on true network-level inactivity (no TCP
        // activity in 300s), NOT on long single-file downloads where
        // bytes are still flowing. Replaces our previous stdout-idle
        // heuristic, which couldn't distinguish "stuck" from "slow big
        // file." Per Perforce docs, this is the right way to detect
        // network stalls.
        "-vnet.maxwait=300",
    ];
    let combined: Vec<&str> = base.iter().copied().chain(p4_args.iter().copied()).collect();
    cmd.args(&combined)
        .env("P4CHARSET", "utf8")
        .current_dir(workspace_root)
        .creation_flags(no_window())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        CompanionError::Perforce(format!("could not spawn p4 {label}: {e}"))
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CompanionError::Perforce("missing stdout pipe".into()))?;
    let stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| CompanionError::Perforce("missing stderr pipe".into()))?;

    // Drain stderr concurrently into a String. Without this, a stderr
    // buffer full would block the child, deadlocking us against our own
    // stdout reader. We need the full stderr text after the child
    // exits so we can distinguish "File(s) up-to-date." (benign) from
    // "Client 'foo' unknown." (real error, was being silently swallowed
    // pre-fix).
    let stderr_handle = tauri::async_runtime::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut buf = Vec::new();
        let mut reader = stderr_pipe;
        let _ = reader.read_to_end(&mut buf).await;
        String::from_utf8_lossy(&buf).to_string()
    });

    let mut reader = BufReader::new(stdout).lines();
    let mut count = 0u32;

    // Stream stdout lines until EOF. No timeout here — see the module
    // comment above SYNC_POST_EOF_TIMEOUT for why. A single large file
    // can legitimately leave stdout silent for tens of minutes while
    // bytes are flowing on the wire; killing on idle would corrupt
    // those long downloads. (-vnet.maxwait above handles true stalls.)
    while let Some(line) = reader.next_line().await.map_err(|e| {
        CompanionError::Perforce(format!("read failed: {e}"))
    })? {
        if !line.trim().is_empty() {
            count += 1;
            on_progress(&line);
        }
    }

    // Stdout closed; the child should exit within milliseconds. Use a
    // small safety timeout in case the process is hung post-EOF.
    let status = timeout(SYNC_POST_EOF_TIMEOUT, child.wait())
        .await
        .map_err(|_| {
            CompanionError::Perforce(format!(
                "{label} finished streaming but the p4 process didn't exit"
            ))
        })?
        .map_err(|e| CompanionError::Perforce(format!("{label} wait failed: {e}")))?;

    // Read the captured stderr now that the child has exited.
    let stderr_text = stderr_handle.await.unwrap_or_default();

    if !status.success() {
        // p4 sync exit 1 + stderr containing "file(s) up-to-date" is the
        // benign "nothing changed" case. ANY other stderr at a non-zero
        // exit is a real failure — e.g. "Client 'foo' unknown.",
        // "Path 'bar' is not under client's root.", "Authentication
        // failed." — and was being silently swallowed by the previous
        // unconditional Ok(count) fall-through. Surface stderr verbatim
        // so translate() can map it to a friendly error.
        let lc = stderr_text.to_lowercase();
        if lc.contains("file(s) up-to-date") {
            return Ok(count);
        }
        let msg = if !stderr_text.trim().is_empty() {
            stderr_text.trim().to_string()
        } else {
            format!("{label} exited with status {:?}", status.code())
        };
        return Err(CompanionError::Perforce(msg));
    }
    Ok(count)
}

// ----- Tauri commands -----

#[tauri::command]
pub async fn p4_probe<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    server: String,
    user: String,
    ticket: String,
) -> Result<ServerInfo, CompanionError> {
    let p4_exe = PathBuf::from(paths::p4_exe(&app).map_err(CompanionError::Perforce)?);
    if !p4_exe.exists() {
        return Err(CompanionError::Perforce(format!(
            "bundled p4.exe missing: {}",
            p4_exe.display()
        )));
    }
    let cfg = P4Config { server, user, ticket, p4_exe };
    info(&cfg).await
}

/// Resolve a project_id → P4Config by reading the persisted project state.
fn project_config<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    project_id: &str,
) -> Result<(P4Config, String, PathBuf), CompanionError> {
    let state = app.state::<super::projects::ProjectState>();
    let projects = state.snapshot();
    let project = projects
        .iter()
        .find(|p| p.project_id == project_id)
        .ok_or_else(|| CompanionError::Other(format!("no project {project_id}")))?
        .clone();
    let p4_exe = PathBuf::from(paths::p4_exe(app).map_err(CompanionError::Perforce)?);
    let cfg = P4Config {
        server: project.server,
        user: project.user,
        ticket: project.ticket,
        p4_exe,
    };
    let workspace_root = PathBuf::from(project.workspace_root);
    Ok((cfg, project.workspace_name, workspace_root))
}

#[tauri::command]
pub async fn p4_list_changes<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    project_id: String,
) -> Result<Vec<ChangedFile>, CompanionError> {
    let (cfg, workspace, root) = project_config(&app, &project_id)?;
    list_changes(&cfg, &workspace, &root).await
}

#[tauri::command]
pub async fn p4_change_counts<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    project_id: String,
) -> Result<ChangeCounts, CompanionError> {
    let (cfg, workspace, root) = project_config(&app, &project_id)?;
    change_counts(&cfg, &workspace, &root).await
}

#[tauri::command]
pub async fn p4_submit_changes<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    project_id: String,
    files: Vec<String>,
    description: String,
) -> Result<i32, CompanionError> {
    let (cfg, workspace, root) = project_config(&app, &project_id)?;
    submit_changes(&cfg, &workspace, &root, &files, &description).await
}

#[tauri::command]
pub async fn p4_restore_file<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    project_id: String,
    local_path: String,
) -> Result<(), CompanionError> {
    let (cfg, workspace, root) = project_config(&app, &project_id)?;
    restore_file(&cfg, &workspace, &root, &local_path).await
}

/// RAII guard that marks a project as "pulling" in TrayPollState for
/// the duration of a long Perforce op (pull/force-resync/repair/relocate).
/// On Drop the project is unmarked, so the tray poll loop can resume
/// running `change_counts` against the workspace. Covers both the Ok and
/// Err return paths, plus any early `?` propagation.
struct PullingGuard<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
    project_id: String,
}

impl<R: tauri::Runtime> PullingGuard<R> {
    fn new(app: tauri::AppHandle<R>, project_id: String) -> Self {
        if let Some(state) = app.try_state::<super::tray_poll::TrayPollState>() {
            state.mark_pulling(&project_id);
        }
        Self { app, project_id }
    }
}

impl<R: tauri::Runtime> Drop for PullingGuard<R> {
    fn drop(&mut self) {
        if let Some(state) = self.app.try_state::<super::tray_poll::TrayPollState>() {
            state.unmark_pulling(&self.project_id);
        }
    }
}

#[tauri::command]
pub async fn p4_pull_latest<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    project_id: String,
) -> Result<u32, CompanionError> {
    let _pulling = PullingGuard::new(app.clone(), project_id.clone());
    let (cfg, workspace, root) = project_config(&app, &project_id)?;
    let app_for_progress = app.clone();
    sync_workspace(&cfg, &workspace, &root, move |line| {
        let _ = app_for_progress.emit(
            "pull-progress",
            serde_json::json!({ "project_id": project_id, "line": line, "source": "main" }),
        );
    })
    .await
}

#[tauri::command]
pub async fn p4_force_resync<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    project_id: String,
) -> Result<u32, CompanionError> {
    let _pulling = PullingGuard::new(app.clone(), project_id.clone());
    let (cfg, workspace, root) = project_config(&app, &project_id)?;
    let app_for_progress = app.clone();
    let pid_for_event = project_id.clone();
    force_resync(&cfg, &workspace, &root, move |line| {
        let _ = app_for_progress.emit(
            "pull-progress",
            serde_json::json!({ "project_id": pid_for_event, "line": line, "force": true, "source": "main" }),
        );
    })
    .await
}

#[tauri::command]
pub async fn p4_repair_workspace<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    project_id: String,
) -> Result<u32, CompanionError> {
    let _pulling = PullingGuard::new(app.clone(), project_id.clone());
    let (cfg, workspace, root) = project_config(&app, &project_id)?;
    let app_for_progress = app.clone();
    let pid_for_event = project_id.clone();
    repair_workspace(&cfg, &workspace, &root, move |line| {
        let _ = app_for_progress.emit(
            "pull-progress",
            serde_json::json!({ "project_id": pid_for_event, "line": line, "repair": true, "source": "main" }),
        );
    })
    .await
}

/// Validate a path argument before handing it to explorer.exe.
///
/// `explorer.exe /select,<path>` is sensitive to the shape of the path
/// — a stray comma, an interpreted leading "/" flag, or a non-existent
/// path can cause Explorer to either open the wrong location or
/// misinterpret the rest of the command line. Lock the input down by
/// requiring a canonical absolute path that:
///   - parses as a Path
///   - is absolute (has a Windows drive prefix, NOT starting with "/")
///   - canonicalizes successfully (catches "..\..\foo" smuggling)
///   - actually exists on disk
///
/// Returns the canonical PathBuf on success. Errors describe what
/// failed in user-facing language.
pub(crate) fn validate_explorer_path(input: &str) -> Result<PathBuf, CompanionError> {
    let path = Path::new(input);
    if input.starts_with('/') || input.starts_with('\\') && !input.starts_with("\\\\") {
        // Reject leading slash (could be misread as an explorer.exe flag)
        // but allow UNC paths which start with "\\".
        return Err(CompanionError::Other(format!(
            "refusing to open suspicious path: {input}"
        )));
    }
    if !path.is_absolute() {
        return Err(CompanionError::Other(format!(
            "refusing to open non-absolute path: {input}"
        )));
    }
    let canonical = std::fs::canonicalize(path).map_err(|e| {
        CompanionError::Other(format!("path does not exist or is unreadable ({input}): {e}"))
    })?;
    Ok(canonical)
}

#[tauri::command]
pub async fn p4_reveal_in_explorer(local_path: String) -> Result<(), CompanionError> {
    use std::os::windows::process::CommandExt;
    use std::process::Command as StdCommand;

    let canonical = validate_explorer_path(&local_path)?;
    let cleaned = strip_canonical_prefix(&canonical);

    // /select,<path> is the Explorer convention: a SINGLE argument with
    // the path glued after the comma. Despite the comma, this works
    // because Explorer parses "/select," as a flag prefix and consumes
    // the rest of the argument as the path. std::process::Command on
    // Windows applies CRT-style argv quoting, so embedded spaces survive.
    StdCommand::new("explorer.exe")
        .arg(format!("/select,{}", cleaned))
        .creation_flags(no_window())
        .spawn()
        .map_err(|e| CompanionError::Other(format!("could not open explorer: {e}")))?;
    Ok(())
}

/// Strip the verbatim prefixes that `std::fs::canonicalize` produces on
/// Windows so explorer.exe (which interprets them literally and opens an
/// empty window) gets a normal-looking path.
///
/// Local drives:  `\\?\C:\Foo` → `C:\Foo`
/// UNC paths:     `\\?\UNC\server\share\path` → `\\server\share\path`
pub(crate) fn strip_canonical_prefix(canonical: &Path) -> String {
    let s = canonical.to_string_lossy().to_string();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        // Re-add the double-backslash UNC sentinel.
        return format!(r"\\{}", rest);
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        return rest.to_string();
    }
    s
}
