//! Resolves where bundled binaries live at runtime and where Companion
//! stores its own state.
//!
//! Layout:
//!  - `p4.exe` is bundled into Companion's own bin dir
//!    (`%LOCALAPPDATA%\Spaceshop\Companion\bin\p4.exe`) on first run.
//!  - `tailscale.exe` lives at Tailscale's standard install path
//!    (`C:\Program Files\Tailscale\tailscale.exe`) — Tailscale ships only
//!    an installer, not a redistributable binary, so we run the bundled
//!    `tailscale.msi` silently on first launch and use the installed path.

use std::path::PathBuf;

use tauri::Manager;

pub fn app_local_data_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map_err(|e| format!("could not resolve app local data dir: {e}"))
}

/// %LOCALAPPDATA%\Spaceshop\Companion\bin\  — where p4.exe lives after
/// first-run copy from the bundled resources.
pub fn bin_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    let d = app_local_data_dir(app)?.join("bin");
    std::fs::create_dir_all(&d).map_err(|e| format!("could not create bin dir: {e}"))?;
    Ok(d)
}

pub fn p4_exe<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    Ok(bin_dir(app)?.join("p4.exe"))
}

/// Tailscale's standard install location. The Tailscale MSI installs
/// here unconditionally; if a user has installed Tailscale themselves
/// it's also here. We do NOT relocate Tailscale's binaries.
pub fn tailscale_exe<R: tauri::Runtime>(_app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    Ok(PathBuf::from(r"C:\Program Files\Tailscale\tailscale.exe"))
}

/// `%USERPROFILE%\p4tickets.txt` — the default location Perforce CLI and
/// Unreal's Source Control plugin both read.
pub fn p4tickets_path() -> Result<PathBuf, String> {
    dirs::home_dir()
        .map(|h| h.join("p4tickets.txt"))
        .ok_or_else(|| "could not resolve user profile".into())
}

/// Where in the Tauri bundle the source-of-truth binaries + MSI live.
pub fn resource_binaries_dir<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<PathBuf, String> {
    app.path()
        .resource_dir()
        .map(|d| d.join("binaries"))
        .map_err(|e| format!("could not resolve resource dir: {e}"))
}

pub fn tailscale_msi_resource<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<PathBuf, String> {
    Ok(resource_binaries_dir(app)?.join("tailscale.msi"))
}
