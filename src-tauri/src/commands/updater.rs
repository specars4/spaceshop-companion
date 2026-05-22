//! Update check + install via tauri-plugin-updater.
//!
//! On launch Companion spawns a background task that polls the configured
//! endpoint (GitHub Releases for v0.5.1; see `docs/RELEASING.md` for the
//! migration plan to NAS hosting). If a newer version is available, it
//! emits an `update-available` event the frontend listens for to show a
//! banner. The user clicks Install → the `install_update` command runs
//! `download_and_install` then restarts the app.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};
use tauri_plugin_updater::UpdaterExt;
use tracing::{info, warn};

use super::errors::CompanionError;

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub new_version: String,
    pub notes: Option<String>,
}

/// Returns true if the configured updater endpoint is still pointing at
/// a placeholder (e.g. `SPACESHOP_GH_ORG`) rather than a real GitHub org.
/// When true we skip the auto-check entirely — there's no useful work to
/// do and we'd just spam the log with confusing "endpoint did not respond"
/// errors every launch.
fn endpoint_is_placeholder<R: Runtime>(app: &AppHandle<R>) -> bool {
    let cfg = app.config();
    let plugins = &cfg.plugins;
    let Some(updater_cfg) = plugins.0.get("updater") else {
        return true;
    };
    let Some(endpoints) = updater_cfg.get("endpoints").and_then(|e| e.as_array()) else {
        return true;
    };
    endpoints.iter().all(|e| {
        e.as_str().map_or(true, |s| {
            // Heuristic: any of our known placeholder tokens means
            // "not configured yet."
            s.contains("SPACESHOP_GH_ORG")
                || s.contains("YOUR_ORG")
                || s.contains("<your-org>")
        })
    })
}

pub fn spawn_background_check<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        if endpoint_is_placeholder(&app) {
            info!(
                "skipping auto update-check: endpoint not configured yet \
                 (placeholder in tauri.conf.json — see docs/RELEASING.md)"
            );
            return;
        }

        // Small delay so update-check doesn't race with onboarding network calls.
        tokio::time::sleep(std::time::Duration::from_secs(8)).await;

        let updater = match app.updater() {
            Ok(u) => u,
            Err(e) => {
                warn!("updater plugin unavailable: {e}");
                return;
            }
        };

        match updater.check().await {
            Ok(Some(update)) => {
                let info = UpdateInfo {
                    current_version: update.current_version.to_string(),
                    new_version: update.version.to_string(),
                    notes: update.body.clone(),
                };
                info!(
                    "update available: {} -> {}",
                    info.current_version, info.new_version
                );
                let _ = app.emit("update-available", info);
            }
            Ok(None) => {
                info!("companion is up to date");
            }
            Err(e) => {
                // Network errors, missing endpoint, signature mismatch, etc.
                // All non-fatal — user can keep using the current version.
                warn!("update check failed: {e}");
            }
        }
    });
}

#[tauri::command]
pub async fn check_for_updates<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<UpdateInfo>, CompanionError> {
    if endpoint_is_placeholder(&app) {
        return Err(CompanionError::Other(
            "update endpoint not configured yet (placeholder in tauri.conf.json)".into(),
        ));
    }
    let updater = app
        .updater()
        .map_err(|e| CompanionError::Other(format!("updater unavailable: {e}")))?;
    match updater.check().await {
        Ok(Some(update)) => Ok(Some(UpdateInfo {
            current_version: update.current_version.to_string(),
            new_version: update.version.to_string(),
            notes: update.body.clone(),
        })),
        Ok(None) => Ok(None),
        Err(e) => Err(CompanionError::Other(format!("update check failed: {e}"))),
    }
}

#[tauri::command]
pub async fn install_update<R: Runtime>(
    app: AppHandle<R>,
) -> Result<(), CompanionError> {
    let updater = app
        .updater()
        .map_err(|e| CompanionError::Other(format!("updater unavailable: {e}")))?;
    let update = updater
        .check()
        .await
        .map_err(|e| CompanionError::Other(format!("update check failed: {e}")))?
        .ok_or_else(|| CompanionError::Other("no update available".into()))?;

    let app_for_progress = app.clone();
    update
        .download_and_install(
            move |chunk_len, total| {
                // Emit progress to the frontend banner.
                let _ = app_for_progress.emit(
                    "update-progress",
                    serde_json::json!({ "chunk": chunk_len, "total": total }),
                );
            },
            || {
                info!("update downloaded; preparing to install");
            },
        )
        .await
        .map_err(|e| CompanionError::Other(format!("install failed: {e}")))?;

    info!("update installed; restarting");
    app.restart();
}
