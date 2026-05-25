//! Clean uninstall.
//!
//! "Add/Remove Programs" alone leaves a trail of Spaceshop-specific state
//! behind on the contractor's machine — none of it MSI-tracked because
//! Companion writes most of it at runtime. This module is the in-app
//! "Clean uninstall" button's backend: it scrubs that trail and then
//! kicks off the MSI's own uninstaller as a final step.
//!
//! Removed (in order):
//!   1. Our line in `%USERPROFILE%\p4tickets.txt` (other apps' tickets stay)
//!   2. `%APPDATA%\Spaceshop\`        — tauri-plugin-store state, etc.
//!   3. `%LOCALAPPDATA%\Spaceshop\`   — bundled p4.exe, projects.json, flags
//!   4. (Optional) Tailscale — only if the operator opted in. The default
//!      is set by `detect_tailscale_origin` based on whether we wrote the
//!      install-flag at MSI-install time.
//!   5. Companion itself — fire `msiexec /x {ProductCode} /passive` and
//!      exit, OR (if we can't find our own ProductCode) deep-link the
//!      user to Settings → Apps & Features and have them click Uninstall
//!      from there.
//!
//! Best-effort throughout: a locked file or a missing registry entry
//! warns and continues. Anything we can't remove gets logged so the
//! operator can see what happened in a support thread.

use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{info, warn};

use super::errors::CompanionError;
use super::paths;
use super::tailscale::TAILSCALE_INSTALL_FLAG;

/// Companion's bundle identifier, mirrors tauri.conf.json. Used both
/// for the Registry DisplayName lookup and the ms-settings fallback URI.
const COMPANION_PRODUCT_DISPLAY_NAME: &str = "Spaceshop Companion";

/// Tailscale's standard install path. Mirrored from paths::tailscale_exe
/// so we can detect it BEFORE we have an app handle (the React frontend
/// shows the modal before calling clean_uninstall, and detect_origin
/// runs at modal open).
const TAILSCALE_EXE_PATH: &str = r"C:\Program Files\Tailscale\tailscale.exe";

/// Where Tailscale came from, from Companion's point of view. Drives
/// the default state of the "Also uninstall Tailscale" checkbox in
/// the UninstallConfirm modal.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TailscaleOrigin {
    /// Companion's install-flag is present at the expected path. We
    /// installed it; default the checkbox ON.
    InstalledByUs,
    /// Tailscale is on the box but we have no record of installing it.
    /// Checkbox defaults OFF — safer to leave it alone.
    PreExisting,
    /// `tailscale.exe` doesn't exist. Don't show the checkbox at all.
    NotInstalled,
    /// Something went wrong while probing — registry read failed, weird
    /// permissions, etc. Treat like PreExisting (don't touch it).
    Unknown,
}

/// Probe the box for Tailscale's installation state.
///
/// Decision tree:
///   1. `C:\Program Files\Tailscale\tailscale.exe` missing → NotInstalled
///   2. Install-flag at `%LOCALAPPDATA%\Spaceshop\Companion\tailscale-installed-by-us.flag`
///      present → InstalledByUs (high confidence)
///   3. Heuristic fallback: read `HKLM:\…\Uninstall` for entries whose
///      DisplayName matches "Tailscale". If its InstallDate equals
///      Companion's own InstallDate (same YYYYMMDD), call it InstalledByUs.
///      Otherwise PreExisting.
///   4. Any registry/PowerShell failure → Unknown.
///
/// "Same day" definition: Add/Remove Programs InstallDate is an 8-char
/// YYYYMMDD string with no time-of-day. We compare those strings
/// verbatim — string equality on YYYYMMDD is "same calendar day in the
/// machine's local timezone at install time", which is the strongest
/// guarantee the data supports. We do NOT widen the window (±1 day)
/// because midnight-installation collisions are rare AND the cost of
/// a false positive (uninstalling someone else's Tailscale they happen
/// to have installed the day they ran our installer) is worse than
/// the cost of a false negative (leaving Tailscale installed when we
/// could have removed it — they can uninstall it themselves later).
pub async fn detect_tailscale_origin<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> TailscaleOrigin {
    // 1. Tailscale not installed at all?
    let ts_exe = std::path::Path::new(TAILSCALE_EXE_PATH);
    if !ts_exe.exists() {
        return TailscaleOrigin::NotInstalled;
    }

    // 2. Our flag file?
    if let Ok(local_dir) = paths::app_local_data_dir(app) {
        if local_dir.join(TAILSCALE_INSTALL_FLAG).exists() {
            return TailscaleOrigin::InstalledByUs;
        }
    }

    // 3. Heuristic: compare InstallDate of Tailscale vs Companion in the
    //    Uninstall registry. Both keys live in HKLM and per-user HKCU; we
    //    look in both for each app (Tailscale installs into HKLM via MSI;
    //    Companion's NSIS installer can land in either depending on the
    //    elevation context).
    match read_install_dates_via_powershell().await {
        Ok(dates) => {
            let ts_date = dates.tailscale_install_date.as_deref();
            let our_date = dates.companion_install_date.as_deref();
            match (ts_date, our_date) {
                (Some(a), Some(b)) if !a.is_empty() && a == b => {
                    info!(
                        "tailscale install date {a} matches Companion's; \
                         treating as InstalledByUs"
                    );
                    TailscaleOrigin::InstalledByUs
                }
                (Some(_), Some(_)) => TailscaleOrigin::PreExisting,
                // Tailscale registry entry exists but Companion's doesn't —
                // probably means we're running from a dev build, not an
                // installed MSI. Safer to call it PreExisting.
                (Some(_), None) => TailscaleOrigin::PreExisting,
                // No Tailscale registry entry at all yet tailscale.exe is
                // on disk — unusual but possible (someone dropped the
                // exe in by hand?). Don't claim it.
                _ => TailscaleOrigin::PreExisting,
            }
        }
        Err(e) => {
            warn!("could not read Uninstall registry: {e}");
            TailscaleOrigin::Unknown
        }
    }
}

/// Bundle of registry findings we care about during origin detection.
#[derive(Debug, Default)]
struct InstallDates {
    /// YYYYMMDD or empty.
    tailscale_install_date: Option<String>,
    companion_install_date: Option<String>,
}

/// Shell out to PowerShell to walk the Uninstall registry. We avoid
/// linking a `winreg` crate just for this one call — keeps Cargo.toml
/// quiet and reuses the same elevated-PowerShell muscle memory the
/// MSI install path uses.
///
/// Walks both the 64-bit and 32-bit Uninstall hives (a contractor on
/// Windows-on-ARM or with WoW64-installed apps will have entries in
/// the 32-bit one) plus HKCU for per-user installs.
async fn read_install_dates_via_powershell() -> Result<InstallDates, String> {
    // PowerShell script: enumerate uninstall hives, emit JSON with one
    // record per (DisplayName, InstallDate). We pick the Tailscale and
    // Spaceshop Companion lines out of that on the Rust side rather
    // than baking name-matching into the PS — simpler to debug if a
    // future Tailscale rename ("Tailscale Inc.", etc.) breaks the
    // match.
    let script = r#"
$paths = @(
    'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall'
)
$out = @()
foreach ($p in $paths) {
    if (-not (Test-Path $p)) { continue }
    Get-ChildItem $p -ErrorAction SilentlyContinue | ForEach-Object {
        try {
            $props = Get-ItemProperty $_.PsPath -ErrorAction Stop
            $name = $props.DisplayName
            $date = $props.InstallDate
            if ($null -ne $name) {
                $out += [pscustomobject]@{
                    name = [string]$name
                    date = [string]$date
                }
            }
        } catch {}
    }
}
$out | ConvertTo-Json -Compress
"#;

    let mut cmd = Command::new("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ])
    .creation_flags(0x08000000) // CREATE_NO_WINDOW
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

    let child = cmd
        .spawn()
        .map_err(|e| format!("could not spawn powershell: {e}"))?;
    let output = timeout(Duration::from_secs(30), child.wait_with_output())
        .await
        .map_err(|_| "registry walk timed out".to_string())?
        .map_err(|e| format!("wait failed: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "powershell exit {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if stdout.trim().is_empty() {
        return Ok(InstallDates::default());
    }

    // ConvertTo-Json emits a single object if there's exactly one record,
    // otherwise an array. Normalize.
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("could not parse PS JSON: {e}"))?;
    let entries: Vec<serde_json::Value> = match parsed {
        serde_json::Value::Array(a) => a,
        v @ serde_json::Value::Object(_) => vec![v],
        _ => Vec::new(),
    };

    let mut dates = InstallDates::default();
    for ent in entries {
        let name = ent
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let date = ent
            .get("date")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        // Case-insensitive contains check. Tailscale's DisplayName has
        // historically been just "Tailscale" but defensive-code for a
        // possible "Tailscale Inc." or future versioned naming.
        let name_lc = name.to_lowercase();
        if name_lc.contains("tailscale") && dates.tailscale_install_date.is_none() {
            dates.tailscale_install_date = Some(date.clone());
        }
        if name_lc.contains(&COMPANION_PRODUCT_DISPLAY_NAME.to_lowercase())
            && dates.companion_install_date.is_none()
        {
            dates.companion_install_date = Some(date.clone());
        }
    }
    Ok(dates)
}

/// Strip Companion's `(server=user:ticket)` line(s) from p4tickets.txt
/// without touching other servers/users. Reads every persisted project,
/// removes its specific (server, user) entry. Other apps' tickets and
/// other Spaceshop servers the user might have personal accounts on
/// stay intact.
fn scrub_p4tickets<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    // Shared with perforce::write_ticket_file to serialize access to
    // %USERPROFILE%\p4tickets.txt. A read-modify-write race here would
    // be unlikely (uninstall is terminal) but the cost of unifying the
    // lock is zero and the cost of a race is a silently corrupted
    // tickets file for OTHER apps that happen to share the file.
    let _guard = super::perforce::TICKET_FILE_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());

    let path = match paths::p4tickets_path() {
        Ok(p) => p,
        Err(e) => {
            warn!("p4tickets.txt path resolution failed: {e}");
            return;
        }
    };
    if !path.exists() {
        info!("p4tickets.txt not present, nothing to scrub");
        return;
    }

    // Pull every (server, user) Companion knows about. If we can't read
    // state, fall back to removing nothing — better than nuking a line
    // we shouldn't.
    let state = app.state::<super::projects::ProjectState>();
    let projects = state.snapshot();

    // Build the set of prefixes to drop: `<server>=<user>:`
    let drop_prefixes: Vec<String> = projects
        .iter()
        .map(|p| format!("{}={}:", p.server, p.user))
        .collect();

    if drop_prefixes.is_empty() {
        info!("no projects on record; leaving p4tickets.txt alone");
        return;
    }

    let existing = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            warn!("could not read {}: {e}", path.display());
            return;
        }
    };
    let mut kept: Vec<String> = Vec::new();
    let mut dropped = 0usize;
    for line in existing.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        if drop_prefixes.iter().any(|p| trimmed.starts_with(p)) {
            dropped += 1;
            continue;
        }
        kept.push(trimmed.to_string());
    }

    // Clear read-only attribute if Perforce set it — same dance as
    // perforce::write_ticket_file.
    if let Ok(meta) = std::fs::metadata(&path) {
        let mut perms = meta.permissions();
        if perms.readonly() {
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(&path, perms);
        }
    }

    if kept.is_empty() {
        // We were the only customer — delete the file entirely so
        // Unreal's "auto-detect tickets" doesn't see an empty stale file.
        if let Err(e) = std::fs::remove_file(&path) {
            warn!("could not remove empty {}: {e}", path.display());
        } else {
            info!("removed p4tickets.txt (all {} entries were ours)", dropped);
        }
    } else {
        let body = kept.join("\n") + "\n";
        if let Err(e) = std::fs::write(&path, body) {
            warn!("could not rewrite {}: {e}", path.display());
        } else {
            info!(
                "scrubbed {} Companion entr{} from p4tickets.txt; {} other lines kept",
                dropped,
                if dropped == 1 { "y" } else { "ies" },
                kept.len()
            );
        }
    }
}

/// Best-effort recursive directory delete. Logs and continues on any
/// failure (a file that's still open, a permission glitch, etc.).
fn best_effort_remove_dir(label: &str, path: &std::path::Path) {
    if !path.exists() {
        info!("{label} not present at {}", path.display());
        return;
    }
    match std::fs::remove_dir_all(path) {
        Ok(()) => info!("removed {label}: {}", path.display()),
        Err(e) => warn!(
            "could not fully remove {label} at {}: {e} \
             (will continue — some files may be locked by Companion itself \
             and will go away on next reboot)",
            path.display()
        ),
    }
}

/// Look up Tailscale's MSI ProductCode in the registry. We need the GUID
/// to drive `msiexec /x {GUID}`. Returns None if we can't find one —
/// caller falls back to skipping the Tailscale uninstall step rather
/// than guessing.
async fn read_tailscale_product_code() -> Result<Option<String>, String> {
    let script = r#"
$paths = @(
    'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
)
foreach ($p in $paths) {
    if (-not (Test-Path $p)) { continue }
    Get-ChildItem $p -ErrorAction SilentlyContinue | ForEach-Object {
        try {
            $props = Get-ItemProperty $_.PsPath -ErrorAction Stop
            if ($props.DisplayName -and ([string]$props.DisplayName).ToLower().Contains('tailscale')) {
                # The key name itself IS the ProductCode for MSI-installed apps.
                Write-Output $_.PSChildName
                return
            }
        } catch {}
    }
}
"#;

    let mut cmd = Command::new("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ])
    .creation_flags(0x08000000)
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

    let child = cmd
        .spawn()
        .map_err(|e| format!("could not spawn powershell: {e}"))?;
    let output = timeout(Duration::from_secs(30), child.wait_with_output())
        .await
        .map_err(|_| "product-code lookup timed out".to_string())?
        .map_err(|e| format!("wait failed: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        return Ok(None);
    }
    // Some Tailscale builds register without an MSI ProductCode-style key
    // name (e.g. "Tailscale" instead of "{GUID}"). Only return what
    // actually looks like a brace-wrapped GUID; otherwise None and we'll
    // fall back to skipping.
    let first = stdout.lines().next().unwrap_or("").trim().to_string();
    if first.starts_with('{') && first.ends_with('}') {
        Ok(Some(first))
    } else {
        info!("tailscale uninstall key is non-MSI ({first}); cannot use msiexec");
        Ok(None)
    }
}

/// Same as `read_tailscale_product_code` but for Companion itself. Used
/// to fire `msiexec /x` against our own MSI as the very last step. If
/// the registry doesn't have a brace-GUID for Spaceshop Companion (e.g.
/// we shipped via NSIS, or the user is running an unpackaged dev build),
/// we return None and let the caller fall back to opening
/// `ms-settings:appsfeatures`.
async fn read_companion_product_code() -> Result<Option<String>, String> {
    let script = format!(
        r#"
$paths = @(
    'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall'
)
foreach ($p in $paths) {{
    if (-not (Test-Path $p)) {{ continue }}
    Get-ChildItem $p -ErrorAction SilentlyContinue | ForEach-Object {{
        try {{
            $props = Get-ItemProperty $_.PsPath -ErrorAction Stop
            if ($props.DisplayName -and ([string]$props.DisplayName).ToLower().Contains('{needle}')) {{
                Write-Output $_.PSChildName
                return
            }}
        }} catch {{}}
    }}
}}
"#,
        needle = COMPANION_PRODUCT_DISPLAY_NAME.to_lowercase()
    );

    let mut cmd = Command::new("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        &script,
    ])
    .creation_flags(0x08000000)
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

    let child = cmd
        .spawn()
        .map_err(|e| format!("could not spawn powershell: {e}"))?;
    let output = timeout(Duration::from_secs(30), child.wait_with_output())
        .await
        .map_err(|_| "companion-product-code lookup timed out".to_string())?
        .map_err(|e| format!("wait failed: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        return Ok(None);
    }
    let first = stdout.lines().next().unwrap_or("").trim().to_string();
    if first.starts_with('{') && first.ends_with('}') {
        Ok(Some(first))
    } else {
        Ok(None)
    }
}

/// Run `msiexec /x <GUID> /qn /norestart` under elevation. Mirrors the
/// install path in `tailscale.rs::install_via_msi` so the UAC prompt,
/// arg-quoting, and exit-code handling all use code we already trust.
async fn run_msiexec_uninstall(product_code: &str) -> Result<i32, String> {
    let ps_command = format!(
        "$p = Start-Process -FilePath 'msiexec.exe' -ArgumentList '/x','{}','/qn','/norestart' -Verb RunAs -PassThru; $p.WaitForExit(); $p.ExitCode",
        product_code
    );

    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &ps_command])
        .creation_flags(0x08000000)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = cmd
        .spawn()
        .map_err(|e| format!("could not spawn msiexec uninstall: {e}"))?;
    let output = timeout(Duration::from_secs(300), child.wait_with_output())
        .await
        .map_err(|_| "msiexec uninstall timed out".to_string())?
        .map_err(|e| format!("wait failed: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let exit: i32 = stdout.parse().unwrap_or(-1);
    Ok(exit)
}

/// Open Settings → Apps & Features so the user can finish removing
/// Companion from the system UI. Last-resort fallback when we can't
/// find our own MSI ProductCode in the registry.
fn open_appsfeatures_settings() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command as StdCommand;
    StdCommand::new("cmd.exe")
        .args(["/C", "start", "", "ms-settings:appsfeatures"])
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|e| format!("could not open Settings: {e}"))?;
    Ok(())
}

/// What we did. Surfaced so the React side can show a final "we kicked
/// off the Windows uninstaller in the background, you can close
/// Companion when ready" message even when we hand off to ms-settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanUninstallResult {
    pub p4tickets_scrubbed: bool,
    pub appdata_removed: bool,
    pub localappdata_removed: bool,
    pub tailscale_uninstalled: bool,
    /// True if we successfully fired `msiexec /x {companion}`.
    /// False if we had to fall back to ms-settings (user has to click
    /// Uninstall manually). Either way Companion will exit shortly.
    pub companion_uninstall_fired: bool,
    /// Set when we couldn't find our own ProductCode and bounced the
    /// user to Settings → Apps & Features instead. Frontend can show
    /// a one-line instruction.
    pub fell_back_to_settings: bool,
}

/// The big red button's backend.
///
/// Sequence (best-effort; failures log and continue except for catastrophic
/// app-handle problems):
///   1. Scrub p4tickets.txt
///   2. Remove %APPDATA%\Spaceshop\
///   3. Remove %LOCALAPPDATA%\Spaceshop\
///   4. If remove_tailscale: msiexec /x {tailscale-product-code} /qn
///   5. Fire msiexec /x {companion-product-code} /passive (or open
///      ms-settings:appsfeatures as fallback)
///   6. Exit the app
///
/// The `app.exit(0)` at the end is what tells Companion to close. The
/// msiexec /x call against our OWN MSI can run concurrently with us
/// being alive — Windows Installer will simply complain about needing
/// to close the app and ask the user, OR /passive mode will silently
/// queue our removal until after we exit. Net effect: Companion's
/// window goes away within ~1s and the MSI uninstaller takes care of
/// the binaries on disk.
pub async fn clean_uninstall<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    remove_tailscale: bool,
) -> Result<CleanUninstallResult, CompanionError> {
    // Signal the tray poll loop to exit BEFORE any destructive op runs.
    // The poll loop reads project state + shells out to p4 every 30s; if
    // we delete %LOCALAPPDATA%\Spaceshop\Companion\bin\p4.exe while a
    // cycle is mid-flight, the next poll either crashes or emits a
    // confusing error notification. We notify_waiters() (which wakes
    // the loop's tokio::select! on shutdown.notified()) and sleep
    // briefly so any in-flight iteration can finish cleanly.
    if let Some(state) = app.try_state::<super::tray_poll::TrayPollState>() {
        state.shutdown.notify_waiters();
    }
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut result = CleanUninstallResult {
        p4tickets_scrubbed: false,
        appdata_removed: false,
        localappdata_removed: false,
        tailscale_uninstalled: false,
        companion_uninstall_fired: false,
        fell_back_to_settings: false,
    };

    // 1. p4tickets.txt — scrub Companion's lines only.
    scrub_p4tickets(app);
    result.p4tickets_scrubbed = true;

    // 2/3. Companion state dirs. We delete the Spaceshop PARENT folder,
    // not just the Companion subfolder — there's nothing else under
    // %APPDATA%\Spaceshop\ today and removing the parent leaves the
    // user's appdata clean rather than with an empty husk folder.
    //
    // %APPDATA%
    if let Some(appdata) = dirs::config_dir() {
        let target = appdata.join("Spaceshop");
        best_effort_remove_dir("%APPDATA%\\Spaceshop", &target);
        result.appdata_removed = !target.exists();
    }
    // %LOCALAPPDATA%
    if let Some(local) = dirs::data_local_dir() {
        let target = local.join("Spaceshop");
        // CAVEAT: bundled p4.exe under
        //   %LOCALAPPDATA%\Spaceshop\Companion\bin\p4.exe
        // might be in use IF a p4 sync is in flight when the user
        // clicks uninstall. best_effort_remove_dir handles that
        // gracefully — Windows leaves the file but removes everything
        // else; we log and continue.
        best_effort_remove_dir("%LOCALAPPDATA%\\Spaceshop", &target);
        result.localappdata_removed = !target.exists();
    }

    // 4. Tailscale, only if the operator opted in. We re-confirm
    //    that the exe still exists before invoking msiexec — if the
    //    user already uninstalled it manually between modal-open and
    //    click-confirm, no-op.
    if remove_tailscale && std::path::Path::new(TAILSCALE_EXE_PATH).exists() {
        match read_tailscale_product_code().await {
            Ok(Some(code)) => match run_msiexec_uninstall(&code).await {
                Ok(0) | Ok(3010) => {
                    info!("tailscale uninstalled via msiexec");
                    result.tailscale_uninstalled = true;
                }
                Ok(other) => {
                    warn!(
                        "tailscale msiexec /x exited {other}; the user may need \
                         to remove it from Settings → Apps & Features themselves"
                    );
                }
                Err(e) => warn!("tailscale msiexec /x failed: {e}"),
            },
            Ok(None) => {
                warn!(
                    "could not find Tailscale's MSI ProductCode in the Uninstall \
                     registry — skipping; user can remove from Settings"
                );
            }
            Err(e) => warn!("tailscale product-code lookup failed: {e}"),
        }
    }

    // 5. Fire OUR uninstaller. /passive shows a progress UI but no
    //    prompts; safer than /qn (which would silently background-uninstall
    //    while the user is staring at Companion's window) because the
    //    progress UI gives them a visible "yes this is happening" signal.
    match read_companion_product_code().await {
        Ok(Some(code)) => {
            // We don't `Verb RunAs` here — Companion is per-user MSI when
            // installed via Tauri's default WiX template, but if it
            // happens to be per-machine the elevation prompt will fire.
            let ps_command = format!(
                "Start-Process -FilePath 'msiexec.exe' -ArgumentList '/x','{}','/passive','/norestart'",
                code
            );
            let mut cmd = Command::new("powershell.exe");
            cmd.args(["-NoProfile", "-NonInteractive", "-Command", &ps_command])
                .creation_flags(0x08000000)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            // Fire-and-forget — we want to exit Companion immediately so
            // msiexec doesn't get stuck "files in use".
            match cmd.spawn() {
                Ok(_) => {
                    info!("kicked off msiexec /x {code} /passive; exiting");
                    result.companion_uninstall_fired = true;
                }
                Err(e) => {
                    warn!("could not spawn companion uninstaller: {e}; opening Settings instead");
                    if open_appsfeatures_settings().is_ok() {
                        result.fell_back_to_settings = true;
                    }
                }
            }
        }
        Ok(None) => {
            // No GUID in the registry — this is the documented fallback
            // path (e.g. NSIS-installed builds, dev builds). Open
            // Settings → Apps & Features so the user can complete the
            // uninstall from the standard Windows UI.
            warn!(
                "could not find Spaceshop Companion in the Uninstall registry — \
                 falling back to ms-settings:appsfeatures so the user can \
                 uninstall from Windows Settings"
            );
            if open_appsfeatures_settings().is_ok() {
                result.fell_back_to_settings = true;
            }
        }
        Err(e) => {
            warn!("companion product-code lookup failed: {e}; opening Settings");
            if open_appsfeatures_settings().is_ok() {
                result.fell_back_to_settings = true;
            }
        }
    }

    // 6. Schedule app exit AFTER the response goes back. The frontend
    //    will navigate to a final "Goodbye" view in the meantime.
    //    100ms delay gives the IPC reply time to make it back to the
    //    webview and the React tree time to render the goodbye state
    //    before the process dies.
    let handle_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(750)).await;
        handle_clone.exit(0);
    });

    Ok(result)
}

// ============================================================================
// Tauri commands
// ============================================================================

#[tauri::command]
pub async fn detect_tailscale_origin_cmd<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<TailscaleOrigin, CompanionError> {
    Ok(detect_tailscale_origin(&app).await)
}

#[tauri::command]
pub async fn clean_uninstall_cmd<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    remove_tailscale: bool,
) -> Result<CleanUninstallResult, CompanionError> {
    clean_uninstall(&app, remove_tailscale).await
}

// Small `tauri::Manager` import needed for `app.state::<...>()` in
// scrub_p4tickets. Re-export it here at module scope so the use stays
// local.
#[allow(unused_imports)]
use tauri::Manager;
