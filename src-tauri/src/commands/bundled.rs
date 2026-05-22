//! First-run setup of bundled binaries.
//!
//! - `p4.exe` is copied from Companion's bundled resources to
//!   `%LOCALAPPDATA%\Spaceshop\Companion\bin\`.
//! - `tailscale.msi` is NOT copied; Companion's onboarding flow invokes
//!   `msiexec` against the resource path itself if Tailscale isn't
//!   detected.
//!
//! Idempotent — re-running is cheap and verifies the binaries are still
//! present (helps recover from antivirus quarantine).

use std::path::PathBuf;

use tracing::{debug, info, warn};

use super::paths;

const COPY_BINARIES: &[&str] = &["p4.exe"];

pub fn ensure_binaries<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<(), String> {
    let src_dir: PathBuf = paths::resource_binaries_dir(app)?;
    let dst_dir: PathBuf = paths::bin_dir(app)?;

    if !src_dir.exists() {
        warn!(
            "resource binaries dir not present at {}; assuming dev mode",
            src_dir.display()
        );
        return Ok(());
    }

    for name in COPY_BINARIES {
        let src = src_dir.join(name);
        let dst = dst_dir.join(name);
        if !src.exists() {
            warn!("bundled binary missing from resources: {}", src.display());
            continue;
        }
        let should_copy = match (std::fs::metadata(&src), std::fs::metadata(&dst)) {
            (Ok(s), Ok(d)) => s.len() != d.len(),
            (Ok(_), Err(_)) => true,
            _ => false,
        };
        if should_copy {
            std::fs::copy(&src, &dst).map_err(|e| {
                format!("could not copy {} → {}: {e}", src.display(), dst.display())
            })?;
            info!("copied bundled binary {} → {}", name, dst.display());
        } else {
            debug!("bundled binary up to date: {}", dst.display());
        }
    }
    Ok(())
}

/// Returns true if Tailscale is already installed at the standard path.
pub fn tailscale_installed<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    paths::tailscale_exe(app)
        .map(|p| p.exists())
        .unwrap_or(false)
}
