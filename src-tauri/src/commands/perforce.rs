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
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};
use tokio::process::Command;
use tokio::time::timeout;

use super::errors::CompanionError;
use super::invite::WorkspaceTemplate;
use super::paths;

const CALL_TIMEOUT: Duration = Duration::from_secs(30);

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

fn no_window() -> u32 {
    0x08000000
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
pub async fn list_changes(
    config: &P4Config,
    workspace: &str,
    workspace_root: &Path,
) -> Result<Vec<ChangedFile>, CompanionError> {
    let (stdout, stderr, code) = run_p4(
        config,
        &["-c", workspace, "status"],
        None,
        Some(workspace_root),
        CALL_TIMEOUT,
    )
    .await?;

    // "File(s) not opened on this client." / empty output → no changes.
    let combined = format!("{stdout}\n{stderr}").trim().to_string();
    if combined.is_empty() || combined.to_lowercase().contains("not opened on this client") {
        if code != 0 {
            // Some versions return exit 1 with that message; treat as empty.
            return Ok(Vec::new());
        }
        return Ok(Vec::new());
    }
    if code != 0 {
        return Err(CompanionError::Perforce(if !stderr.trim().is_empty() {
            stderr
        } else {
            stdout
        }));
    }

    let mut out = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("... ") {
            continue;
        }

        // Try " - reconcile to " first (file not yet opened), then
        // " - submit change <N> to " (file already in a changelist).
        // After splitting, `right` should look like "<action> <depot>#<rev>".
        let (left, right_with_action) = if let Some((l, r)) =
            trimmed.split_once(" - reconcile to ")
        {
            (l, r)
        } else if let Some((l, r)) = trimmed.split_once(" - submit change ") {
            // r looks like "12 to add //depot/foo#1" — strip "<N> to ".
            let after_to = r.split_once(" to ").map(|(_, rest)| rest);
            match after_to {
                Some(rest) => (l, rest),
                None => continue,
            }
        } else {
            continue;
        };

        let local_rel = left.trim();
        let Some((action, depot_with_rev)) = right_with_action.trim().split_once(' ') else {
            continue;
        };
        let status = match action {
            "edit" => "M",
            "add" => "A",
            "delete" => "D",
            _ => continue,
        }
        .to_string();
        let depot_path = depot_with_rev
            .split('#')
            .next()
            .unwrap_or(depot_with_rev)
            .to_string();
        let local_path = workspace_root.join(local_rel).to_string_lossy().into_owned();
        let size = std::fs::metadata(&local_path).ok().map(|m| m.len());
        out.push(ChangedFile {
            status,
            local_path,
            depot_path,
            size,
        });
    }
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
    let local = list_changes(config, workspace, workspace_root).await?;
    let local_pending = local.len() as u32;

    // p4 sync -n //... — dry run, lists files that would be pulled.
    let (stdout, stderr, code) = run_p4(
        config,
        &["-c", workspace, "sync", "-n", "//..."],
        None,
        Some(workspace_root),
        CALL_TIMEOUT,
    )
    .await?;

    let combined = format!("{stdout}\n{stderr}").to_lowercase();
    let remote_unseen = if combined.contains("file(s) up-to-date") {
        0
    } else if code != 0 && !combined.contains(" - ") {
        return Err(CompanionError::Perforce(if !stderr.trim().is_empty() {
            stderr
        } else {
            stdout
        }));
    } else {
        stdout
            .lines()
            .filter(|l| {
                let l = l.trim();
                !l.is_empty() && l.contains(" - ")
            })
            .count() as u32
    };

    Ok(ChangeCounts {
        local_pending,
        remote_unseen,
        folder_missing: false,
    })
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
            serde_json::json!({ "project_id": pid, "line": line, "relocating": true }),
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
        &["-c", workspace, "flush", "//...#0"],
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
    let mut reader = BufReader::new(stdout).lines();
    let mut count = 0u32;

    // Stream stdout lines until EOF. No timeout here — see the module
    // comment above SYNC_POST_EOF_TIMEOUT for why. A single large file
    // can legitimately leave stdout silent for tens of minutes while
    // bytes are flowing on the wire; killing on idle would corrupt
    // those long downloads.
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

    if !status.success() {
        // p4 sync exit 1 with "File(s) up-to-date." on stderr just means
        // nothing changed — not a real failure for our purposes.
        return Ok(count);
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

#[tauri::command]
pub async fn p4_pull_latest<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    project_id: String,
) -> Result<u32, CompanionError> {
    let (cfg, workspace, root) = project_config(&app, &project_id)?;
    let app_for_progress = app.clone();
    sync_workspace(&cfg, &workspace, &root, move |line| {
        let _ = app_for_progress.emit(
            "pull-progress",
            serde_json::json!({ "project_id": project_id, "line": line }),
        );
    })
    .await
}

#[tauri::command]
pub async fn p4_force_resync<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    project_id: String,
) -> Result<u32, CompanionError> {
    let (cfg, workspace, root) = project_config(&app, &project_id)?;
    let app_for_progress = app.clone();
    let pid_for_event = project_id.clone();
    force_resync(&cfg, &workspace, &root, move |line| {
        let _ = app_for_progress.emit(
            "pull-progress",
            serde_json::json!({ "project_id": pid_for_event, "line": line, "force": true }),
        );
    })
    .await
}

#[tauri::command]
pub async fn p4_reveal_in_explorer(local_path: String) -> Result<(), CompanionError> {
    use std::os::windows::process::CommandExt;
    use std::process::Command as StdCommand;
    StdCommand::new("explorer.exe")
        .arg(format!("/select,{}", local_path))
        .creation_flags(no_window())
        .spawn()
        .map_err(|e| CompanionError::Other(format!("could not open explorer: {e}")))?;
    Ok(())
}
