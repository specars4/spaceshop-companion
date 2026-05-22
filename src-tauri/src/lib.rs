mod commands;

use std::sync::Mutex;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;
use tracing::{info, warn};

use commands::projects::ProjectState;

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
        .manage(ProjectState {
            inner: Mutex::new(Vec::new()),
        })
        .setup(|app| {
            // First-run: copy bundled binaries from resources to bin dir.
            if let Err(e) = commands::bundled::ensure_binaries(app.handle()) {
                warn!("bundled binaries setup: {e}");
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

            // Build tray icon. Default icon is whatever the app icon
            // resolves to; status color (green/yellow/red) will be
            // overlaid in Phase 6 polish.
            build_tray(app.handle())?;

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
            commands::perforce::p4_submit_changes,
            commands::perforce::p4_restore_file,
            commands::perforce::p4_pull_latest,
            commands::perforce::p4_force_resync,
            commands::perforce::p4_reveal_in_explorer,
            commands::perforce::p4_relocate_project,
            commands::projects::list_projects,
            commands::projects::remove_project,
            commands::projects::open_project_folder,
            commands::onboarding::apply_invite,
            commands::updater::check_for_updates,
            commands::updater::install_update,
            commands::unreal::open_in_unreal,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Spaceshop Companion");
}

fn load_tray_icon<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> tauri::image::Image<'static> {
    // Look for the custom PNG in two places:
    //  - packaged build: <resources>/icons/tray.png
    //  - dev build: src-tauri/icons/tray.png (relative to cwd when run via `tauri dev`)
    let candidate_paths: Vec<std::path::PathBuf> = {
        let mut v: Vec<std::path::PathBuf> = Vec::new();
        if let Ok(res_dir) = app.path().resource_dir() {
            v.push(res_dir.join("icons").join("tray.png"));
        }
        v.push(std::path::PathBuf::from("icons/tray.png"));
        v.push(std::path::PathBuf::from("src-tauri/icons/tray.png"));
        v
    };

    for p in candidate_paths {
        if p.exists() {
            if let Ok(img) = tauri::image::Image::from_path(&p) {
                return img;
            }
        }
    }

    if let Some(default) = app.default_window_icon() {
        // The borrowed Image needs to be promoted to an owned ('static)
        // value before we can return it.
        return tauri::image::Image::new_owned(
            default.rgba().to_vec(),
            default.width(),
            default.height(),
        );
    }
    tauri::image::Image::new_owned(vec![0; 32 * 32 * 4], 32, 32)
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

fn build_tray<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Companion", true, None::<&str>)?;
    let add = MenuItem::with_id(app, "add", "Add Project…", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&open, &add, &separator, &quit])?;

    // Tray icon resolution order:
    //   1. src-tauri/icons/tray.png   ← drop a custom 32x32 PNG here
    //   2. app's default window icon (icons/icon.ico)
    //   3. an empty placeholder (Tauri requires SOMETHING)
    // The custom path is bundled as a resource via tauri.conf.json so the
    // installed build can find it the same way.
    let tray_icon = load_tray_icon(app);

    let _tray = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .tooltip("Spaceshop Companion")
        .icon(tray_icon)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                    let _ = win.unminimize();
                }
            }
            "add" => {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                    let _ = win.emit("nav", "/onboard");
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Single left-click → open window.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Some(win) = tray.app_handle().get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                    let _ = win.unminimize();
                }
            }
        })
        .build(app)?;
    Ok(())
}

mod plugins {
    pub use tauri_plugin_dialog;
    pub use tauri_plugin_process;
    pub use tauri_plugin_store;
}

#[allow(unused_imports)]
use plugins::*;
