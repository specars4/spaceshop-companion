mod commands;

use std::sync::Mutex;

use tauri::{Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;
use tracing::{info, warn};

use commands::projects::ProjectState;
use commands::tray_poll::{build_initial_tray, build_tinted_icons, spawn_poll_task, TrayPollState};

const DEEP_LINK_EVENT: &str = "deep-link-invite";

// Icons re-baked from restored color SVGs 2026-05-22 — touch this line
// any time icon files change to force tauri::generate_context!() to
// re-embed them into the binary.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    tauri::Builder::default()
        // Single-instance plugin must be the FIRST plugin registered so it
        // can intercept a second launch before any other init runs.
        // When the user double-clicks Companion while it's already
        // running, we focus the existing window instead of starting a
        // new process.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.set_focus();
                let _ = win.unminimize();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .manage(ProjectState {
            inner: Mutex::new(Vec::new()),
        })
        .setup(|app| {
            // First-run: copy bundled binaries from resources to bin dir.
            // If this fails, every downstream p4/tailscale call will spawn-fail
            // with a cryptic "could not spawn p4". Surface a structured
            // startup-error event so the frontend can show a friendly
            // "Companion couldn't unpack its tools, reinstall and try again"
            // banner. Capture the failure on the AppHandle (we can't emit
            // here yet — no webview window listeners are wired up — so we
            // re-emit on a short delay).
            if let Err(e) = commands::bundled::ensure_binaries(app.handle()) {
                warn!("bundled binaries setup failed: {e}");
                let handle_for_err = app.handle().clone();
                let err_msg = e.clone();
                tauri::async_runtime::spawn(async move {
                    // Give the webview a moment to mount listeners.
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    let _ = handle_for_err.emit(
                        "startup-error",
                        serde_json::json!({
                            "kind": "bundled-binaries",
                            "title": "Companion couldn't unpack its tools",
                            "body": "Reinstall Spaceshop Companion and try again.",
                            "details": err_msg,
                        }),
                    );
                });
            }

            // Restore persisted projects.
            let handle_for_state = app.handle().clone();
            match commands::projects::load_persisted(&handle_for_state) {
                Ok(projects) => {
                    info!("loaded {} persisted project(s)", projects.len());
                    handle_for_state
                        .state::<ProjectState>()
                        .replace_all(projects);
                }
                Err(e) => warn!("could not load projects: {e}"),
            }

            // Wire deep-link handler. When Companion is launched via
            // spaceshop-companion://invite/{code}, forward the code to
            // the frontend via a Tauri event so the onboarding screen
            // can pre-fill it.
            let app_handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    let url_str = url.as_str();
                    if let Some(code) = extract_invite_from_url(url_str) {
                        info!("deep link received: invite of {} chars", code.len());
                        let _ = app_handle.emit(DEEP_LINK_EVENT, code);
                        if let Some(win) = app_handle.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                            let _ = win.unminimize();
                        }
                    }
                }
            });

            // Build tray icon. Tinted variants (green/yellow/red) are
            // baked from the source tray.png at startup; the tray-poll
            // task then swaps between them every 30s based on
            // Tailscale + Perforce health. The menu also includes a
            // "Sync <Project>" item per onboarded project and is
            // rebuilt on add/remove.
            let icons = build_tinted_icons(app.handle());
            app.manage(TrayPollState::new(icons));
            build_initial_tray(app.handle())?;
            spawn_poll_task(app.handle().clone());

            // Background update check — emits 'update-available' event
            // when a newer version is published. Frontend shows a banner.
            commands::updater::spawn_background_check(app.handle().clone());

            // Hide window-on-close (X button) — keep Companion alive in
            // the tray. Quit only via tray "Quit".
            if let Some(win) = app.get_webview_window("main") {
                let w = win.clone();
                win.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = w.hide();
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::invite::parse_invite_cmd,
            commands::tailscale::tailscale_status,
            commands::tailscale::tailscale_up,
            commands::tailscale::tailscale_install,
            commands::perforce::p4_probe,
            commands::perforce::p4_list_changes,
            commands::perforce::p4_change_counts,
            commands::perforce::p4_reconcile_workspace,
            commands::perforce::p4_submit_changes,
            commands::perforce::p4_restore_file,
            commands::perforce::p4_pull_latest,
            commands::perforce::p4_force_resync,
            commands::perforce::p4_repair_workspace,
            commands::perforce::p4_reveal_in_explorer,
            commands::perforce::p4_relocate_project,
            commands::projects::list_projects,
            commands::projects::remove_project,
            commands::projects::open_project_folder,
            commands::onboarding::apply_invite,
            commands::updater::check_for_updates,
            commands::updater::install_update,
            commands::unreal::open_in_unreal,
            commands::uninstall::detect_tailscale_origin_cmd,
            commands::uninstall::clean_uninstall_cmd,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Spaceshop Companion");
}

fn extract_invite_from_url(url: &str) -> Option<String> {
    let prefix1 = "spaceshop-companion://invite/";
    let prefix2 = "spaceshop-companion:/invite/";
    if let Some(rest) = url.strip_prefix(prefix1) {
        return Some(rest.to_string());
    }
    if let Some(rest) = url.strip_prefix(prefix2) {
        return Some(rest.to_string());
    }
    None
}

mod plugins {
    pub use tauri_plugin_dialog;
    pub use tauri_plugin_process;
    pub use tauri_plugin_store;
}

#[allow(unused_imports)]
use plugins::*;
