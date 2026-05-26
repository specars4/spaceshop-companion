//! Tray-icon polling + dynamic menu rebuild.
//!
//! v0.6 added live tray status: every 30 seconds we poll Tailscale and each
//! onboarded project's Perforce server, fold the results into a single
//! green/yellow/red health value, and update the tray icon + tooltip
//! accordingly. The tray menu is also rebuilt on project add/remove so
//! the contractor sees a "Sync <Project>" item per project.
//!
//! Design notes:
//!
//! - Icon tints are baked at runtime by reading the source `tray.png`
//!   once and applying a per-pixel color blend. No new crate dep needed —
//!   `tauri::image::Image` already exposes the raw RGBA buffer.
//!
//! - Pulling-state tracking: when a sync starts via the tray, we mark the
//!   project_id in `PullingState` so the next poll cycle skips it. This
//!   keeps a long-running sync from being interrupted by status checks
//!   against the same workspace.
//!
//! - Health aggregation is "worst of all checks." Single-project p4
//!   failures don't blanket-red the tray — they show yellow if Tailscale
//!   is up; red only if Tailscale is also down OR no projects can reach
//!   their server.
//!
//! - The 30s timer is `tauri::async_runtime::spawn`ed and listens on a
//!   `tokio::sync::Notify` for graceful shutdown. Calling
//!   `TrayPollState::shutdown()` wakes the loop early; otherwise the
//!   runtime drops it at app exit.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::Notify;
use tracing::{debug, info, warn};

use super::paths;
use super::perforce::{self, P4Config};
use super::projects::ProjectState;
use super::tailscale;

const POLL_INTERVAL: Duration = Duration::from_secs(30);
const TRAY_ID: &str = "main-tray";

/// Per-tint cache of the tinted tray icons. Built once at startup;
/// `set_icon` clones from here on every health change.
#[derive(Clone)]
pub struct TintedIcons {
    pub neutral: tauri::image::Image<'static>,
    pub green: tauri::image::Image<'static>,
    pub yellow: tauri::image::Image<'static>,
    pub red: tauri::image::Image<'static>,
}

/// Shared state for the polling task. Lives in `tauri::State`.
pub struct TrayPollState {
    pub icons: TintedIcons,
    /// project_ids that have an in-flight long op (sync/repair/etc). The
    /// poller skips per-project p4 checks for these to avoid interfering.
    /// Currently populated by the tray's own "Sync Now" handler — Pull
    /// Latest via the main window doesn't touch this yet (separate
    /// agent's polling owns that path).
    pub pulling: Arc<Mutex<HashSet<String>>>,
    /// **Stacking guard (v0.6.3).** Set to `true` when a poll cycle is
    /// running; the 30 s tick checks this and skips the cycle entirely
    /// if a prior poll is still in flight. Prevents the catastrophic
    /// "Companion piles up six concurrent `p4 status` against an
    /// 8000-file workspace" failure mode that locked out Unreal in
    /// Session 5. Cleared by the poll loop in a `finally`-equivalent
    /// drop guard so it can never get stuck `true` even if the cycle
    /// errors mid-flight.
    pub poll_in_flight: Arc<AtomicBool>,
    /// Notify the poll loop to exit early at app shutdown.
    pub shutdown: Arc<Notify>,
}

impl TrayPollState {
    pub fn new(icons: TintedIcons) -> Self {
        Self {
            icons,
            pulling: Arc::new(Mutex::new(HashSet::new())),
            poll_in_flight: Arc::new(AtomicBool::new(false)),
            shutdown: Arc::new(Notify::new()),
        }
    }

    pub fn mark_pulling(&self, project_id: &str) {
        if let Ok(mut g) = self.pulling.lock() {
            g.insert(project_id.to_string());
        }
    }

    pub fn unmark_pulling(&self, project_id: &str) {
        if let Ok(mut g) = self.pulling.lock() {
            g.remove(project_id);
        }
    }

    pub fn is_pulling(&self, project_id: &str) -> bool {
        self.pulling
            .lock()
            .map(|g| g.contains(project_id))
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayHealth {
    Green,
    Yellow,
    Red,
}

#[derive(Debug, Clone)]
pub struct PollResult {
    pub health: TrayHealth,
    pub tooltip: String,
}

// ---------- Icon tinting ----------

/// Read the base tray PNG and produce neutral + tinted variants.
/// Falls back to a 32x32 solid-color square for any tint we can't compute.
pub fn build_tinted_icons<R: Runtime>(app: &AppHandle<R>) -> TintedIcons {
    let base = load_base_tray_image(app);
    let neutral = base.clone();
    let green = tint_image(&base, (90, 200, 110));
    let yellow = tint_image(&base, (240, 200, 70));
    let red = tint_image(&base, (220, 90, 90));
    TintedIcons {
        neutral,
        green,
        yellow,
        red,
    }
}

fn load_base_tray_image<R: Runtime>(app: &AppHandle<R>) -> tauri::image::Image<'static> {
    let candidates: Vec<PathBuf> = {
        let mut v: Vec<PathBuf> = Vec::new();
        if let Ok(res_dir) = app.path().resource_dir() {
            v.push(res_dir.join("icons").join("tray.png"));
        }
        v.push(PathBuf::from("icons/tray.png"));
        v.push(PathBuf::from("src-tauri/icons/tray.png"));
        v
    };
    for p in candidates {
        if p.exists() {
            if let Ok(img) = tauri::image::Image::from_path(&p) {
                return img.to_owned();
            }
        }
    }
    if let Some(default) = app.default_window_icon() {
        return tauri::image::Image::new_owned(
            default.rgba().to_vec(),
            default.width(),
            default.height(),
        );
    }
    tauri::image::Image::new_owned(vec![0; 32 * 32 * 4], 32, 32)
}

/// Per-pixel tint: blends each opaque pixel toward the tint color using
/// the pixel's luminance as the mix weight. Bright (foreground) pixels
/// pick up most of the tint; dark (background) pixels stay close to
/// their original color. Alpha is preserved so the transparent ring
/// outside the badge stays transparent.
fn tint_image(
    src: &tauri::image::Image<'static>,
    tint: (u8, u8, u8),
) -> tauri::image::Image<'static> {
    let w = src.width();
    let h = src.height();
    let rgba = src.rgba();
    let (tr, tg, tb) = (tint.0 as u16, tint.1 as u16, tint.2 as u16);
    let mut out = Vec::with_capacity(rgba.len());
    let mut i = 0;
    while i + 3 < rgba.len() {
        let r = rgba[i] as u16;
        let g = rgba[i + 1] as u16;
        let b = rgba[i + 2] as u16;
        let a = rgba[i + 3];
        if a == 0 {
            // Fully transparent — copy verbatim.
            out.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            // Luminance weight (0..255). Use Rec. 601 coefficients.
            let lum = ((r * 299 + g * 587 + b * 114) / 1000).min(255);
            let mix = lum; // 0 = no tint, 255 = full tint
            // Blend: out = (1 - w) * src + w * tint, weighted in 0..255.
            let nr = ((r * (255 - mix) + tr * mix) / 255) as u8;
            let ng = ((g * (255 - mix) + tg * mix) / 255) as u8;
            let nb = ((b * (255 - mix) + tb * mix) / 255) as u8;
            out.extend_from_slice(&[nr, ng, nb, a]);
        }
        i += 4;
    }
    tauri::image::Image::new_owned(out, w, h)
}

// ---------- Menu build + rebuild ----------

/// Build the tray's full menu including a "Sync <Project>" item per
/// onboarded project. Items have ids `open`, `add`, `sync-<project_id>`,
/// and `quit`. Re-callable — see `rebuild_menu`.
pub fn build_menu<R: Runtime>(
    app: &AppHandle<R>,
    projects: &[super::projects::Project],
) -> tauri::Result<Menu<R>> {
    let open = MenuItem::with_id(app, "open", "Open Companion", true, None::<&str>)?;
    let add = MenuItem::with_id(app, "add", "Add Project…", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let mut items: Vec<&dyn tauri::menu::IsMenuItem<R>> = Vec::new();
    items.push(&open);
    items.push(&add);

    // Build per-project sync items, keep them alive in this Vec so the
    // references survive `Menu::with_items`.
    let project_items: Vec<MenuItem<R>> = projects
        .iter()
        .map(|p| {
            let id = format!("sync-{}", p.project_id);
            let label = format!("Sync {}", p.project_name);
            MenuItem::with_id(app, &id, &label, true, None::<&str>)
        })
        .collect::<tauri::Result<Vec<_>>>()?;

    if !project_items.is_empty() {
        items.push(&sep1);
        for it in project_items.iter() {
            items.push(it);
        }
    }
    items.push(&sep2);
    items.push(&quit);

    Menu::with_items(app, &items)
}

/// Look up the tray by id and replace its menu with a fresh one
/// containing the current project list. Best-effort — logs but doesn't
/// fail loudly because tray ops aren't critical to app function.
pub fn rebuild_menu<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<ProjectState>();
    let projects = state.snapshot();
    let tray = match app.tray_by_id(TRAY_ID) {
        Some(t) => t,
        None => {
            warn!("tray rebuild: no tray with id {TRAY_ID}");
            return;
        }
    };
    match build_menu(app, &projects) {
        Ok(menu) => {
            if let Err(e) = tray.set_menu(Some(menu)) {
                warn!("tray set_menu failed: {e}");
            } else {
                debug!("tray menu rebuilt with {} project(s)", projects.len());
            }
        }
        Err(e) => warn!("tray build_menu failed: {e}"),
    }
}

// ---------- Polling ----------

/// Spawn the 30s polling task. Idempotent within a single app instance
/// only because it queries the same TrayPollState — caller should call
/// once from `setup()`.
pub fn spawn_poll_task<R: Runtime>(app: AppHandle<R>) {
    let shutdown = {
        let state = app.state::<TrayPollState>();
        state.shutdown.clone()
    };

    tauri::async_runtime::spawn(async move {
        // Initial delay so the rest of startup has a chance to settle
        // before the first network probes fire. Keeps the tray from
        // briefly flashing red on a cold boot where Tailscale hasn't
        // come up yet.
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(3)) => {}
            _ = shutdown.notified() => return,
        }

        loop {
            run_poll_cycle(&app).await;
            tokio::select! {
                _ = tokio::time::sleep(POLL_INTERVAL) => {}
                _ = shutdown.notified() => {
                    info!("tray poll task shutting down");
                    return;
                }
            }
        }
    });
}

async fn run_poll_cycle<R: Runtime>(app: &AppHandle<R>) {
    // v0.6.3 stacking guard. The poll cycle can legitimately take
    // longer than POLL_INTERVAL (30 s) on a real-world Unreal project
    // — `change_counts` walks the whole workspace via `p4 status` +
    // `p4 sync -n`, which we've measured at 1-4 minutes on 8000-file
    // projects. Without this guard, every 30 s tick fired a NEW p4
    // status while the previous one was still running, six in flight
    // simultaneously, each holding a workspace read lock — which
    // starved every other p4 op on the same workspace (Unreal's
    // `p4 edit` blocking for 90+ s, terminal `p4 edit` measured at
    // 117 s). The fix is brutal but correct: if a poll is in flight,
    // SKIP THIS CYCLE. The next tick will retry. A real-time tray
    // icon that stops updating for one tick is fine; a Companion
    // that locks out Unreal is not.
    let in_flight = {
        let state = app.state::<TrayPollState>();
        state.poll_in_flight.clone()
    };
    if in_flight
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        debug!("tray poll: prior cycle still in flight — skipping");
        return;
    }

    // RAII guard so the flag is cleared no matter how we exit (panic,
    // early-return, normal completion). Without this, a single panic
    // inside compute_poll_result would leave the flag stuck true
    // forever and the tray would silently stop polling.
    struct ClearOnDrop(Arc<AtomicBool>);
    impl Drop for ClearOnDrop {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Release);
        }
    }
    let _guard = ClearOnDrop(in_flight);

    let result = compute_poll_result(app).await;
    apply_poll_result(app, &result);
}

/// Compute aggregate health + tooltip text. Public for testing.
pub async fn compute_poll_result<R: Runtime>(app: &AppHandle<R>) -> PollResult {
    let projects = app.state::<ProjectState>().snapshot();

    // 1. Tailscale check.
    let ts_up = matches!(tailscale::status(app).await, Ok(s) if s.up);

    // 2. Per-project p4 reachability + change counts. We use change_counts
    //    because it gives us the "files pending" numbers for the tooltip
    //    AND implicitly probes the server (it calls `p4 status` and
    //    `p4 sync -n`). Skipped if project is mid-pull.
    let poll_state = app.state::<TrayPollState>();
    let mut any_p4_ok = false;
    let mut any_p4_fail = false;
    let mut total_pending: u32 = 0;
    let mut had_query_error = false;

    let p4_exe_path = paths::p4_exe(app).ok().map(PathBuf::from);

    for project in projects.iter() {
        if poll_state.is_pulling(&project.project_id) {
            debug!("skip poll for {} (pulling)", project.project_id);
            continue;
        }
        let Some(ref p4_exe) = p4_exe_path else {
            had_query_error = true;
            continue;
        };
        let cfg = P4Config {
            server: project.server.clone(),
            user: project.user.clone(),
            ticket: project.ticket.clone(),
            p4_exe: p4_exe.clone(),
        };
        let root = std::path::PathBuf::from(&project.workspace_root);
        match perforce::change_counts(&cfg, &project.workspace_name, &root).await {
            Ok(counts) => {
                any_p4_ok = true;
                if !counts.folder_missing {
                    total_pending = total_pending.saturating_add(counts.local_pending);
                    total_pending = total_pending.saturating_add(counts.remote_unseen);
                }
            }
            Err(e) => {
                any_p4_fail = true;
                had_query_error = true;
                debug!("poll: p4 check failed for {}: {e}", project.project_id);
            }
        }
    }

    let has_projects = !projects.is_empty();

    // 3. Health aggregation.
    //    Green: Tailscale up AND (no projects OR at least one p4 ok AND no p4 fail)
    //    Yellow: Partial — TS up but some p4 fail; OR TS down but at least one p4 ok
    //    Red: TS down AND all p4 fail (or no projects + TS down)
    let health = if !has_projects {
        if ts_up {
            TrayHealth::Green
        } else {
            TrayHealth::Yellow
        }
    } else if ts_up && any_p4_ok && !any_p4_fail {
        TrayHealth::Green
    } else if !ts_up && !any_p4_ok {
        TrayHealth::Red
    } else {
        TrayHealth::Yellow
    };

    // 4. Tooltip text.
    let mut tooltip = match projects.len() {
        0 => "Spaceshop Companion".to_string(),
        1 => format!("Spaceshop Companion — {} changes pending", total_pending),
        n => format!(
            "Spaceshop Companion — {} projects · {} total changes pending",
            n, total_pending
        ),
    };
    if had_query_error && has_projects {
        tooltip.push_str(" (some status checks failed)");
    }

    PollResult { health, tooltip }
}

fn apply_poll_result<R: Runtime>(app: &AppHandle<R>, result: &PollResult) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let icons = {
        let state = app.state::<TrayPollState>();
        state.icons.clone()
    };
    let icon = match result.health {
        TrayHealth::Green => icons.green,
        TrayHealth::Yellow => icons.yellow,
        TrayHealth::Red => icons.red,
    };
    if let Err(e) = tray.set_icon(Some(icon)) {
        warn!("tray set_icon failed: {e}");
    }
    if let Err(e) = tray.set_tooltip(Some(&result.tooltip)) {
        warn!("tray set_tooltip failed: {e}");
    }
}

// ---------- Tray-driven sync ----------

/// Triggered from the "Sync <Project>" menu item. Runs the same pipeline
/// as `p4_pull_latest` but without going through a Tauri command — we're
/// already in-process. Marks the project as pulling for the poll loop's
/// benefit, emits the standard `pull-progress` events so the main window
/// (if open) reflects progress, and notifies on completion.
pub fn trigger_tray_sync<R: Runtime>(app: AppHandle<R>, project_id: String) {
    // Re-entry guard: if a sync is already in flight for this project
    // (whether from a previous tray click or a main-window pull), drop
    // this trigger on the floor. Check-and-set runs SYNCHRONOUSLY here,
    // before we spawn anything, so two rapid tray clicks can't both
    // pass the check. The async task below is responsible for unmarking.
    {
        let state = app.state::<TrayPollState>();
        if state.is_pulling(&project_id) {
            info!("tray sync ignored: {} already pulling", project_id);
            return;
        }
        state.mark_pulling(&project_id);
    }

    tauri::async_runtime::spawn(async move {
        let project = {
            let state = app.state::<ProjectState>();
            state
                .snapshot()
                .into_iter()
                .find(|p| p.project_id == project_id)
        };
        let Some(project) = project else {
            warn!("tray sync: no project with id {project_id}");
            // Unmark before we bail so the project isn't permanently stuck.
            app.state::<TrayPollState>().unmark_pulling(&project_id);
            return;
        };
        let p4_exe = match paths::p4_exe(&app) {
            Ok(s) => PathBuf::from(s),
            Err(e) => {
                warn!("tray sync: p4 exe unavailable: {e}");
                app.state::<TrayPollState>().unmark_pulling(&project_id);
                return;
            }
        };
        let cfg = P4Config {
            server: project.server.clone(),
            user: project.user.clone(),
            ticket: project.ticket.clone(),
            p4_exe,
        };
        let root = std::path::PathBuf::from(&project.workspace_root);

        // Forward progress to the main window in case it's open.
        let app_for_progress = app.clone();
        let pid_for_event = project_id.clone();
        let result = perforce::sync_workspace(
            &cfg,
            &project.workspace_name,
            &root,
            move |line| {
                use tauri::Emitter;
                let _ = app_for_progress.emit(
                    "pull-progress",
                    serde_json::json!({
                        "project_id": pid_for_event,
                        "line": line,
                        "source": "tray",
                    }),
                );
            },
        )
        .await;

        {
            let state = app.state::<TrayPollState>();
            state.unmark_pulling(&project_id);
        }

        match result {
            Ok(n) => {
                info!("tray sync done for {}: {n} file(s)", project.project_name);
                notify_sync_done(&app, &project.project_name, n);
            }
            Err(e) => {
                warn!("tray sync failed for {}: {e}", project.project_name);
                notify_sync_failed(&app, &project.project_name, &e.to_string());
            }
        }

        // Force an immediate poll-result refresh so the tooltip catches up.
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            run_poll_cycle(&app2).await;
        });
    });
}

fn notify_sync_done<R: Runtime>(app: &AppHandle<R>, project: &str, n: u32) {
    use tauri_plugin_notification::NotificationExt;
    let body = if n == 0 {
        format!("{project} is already up to date.")
    } else {
        format!("{project}: pulled {n} file(s).")
    };
    let res = app
        .notification()
        .builder()
        .title("Sync complete")
        .body(body)
        .show();
    if let Err(e) = res {
        debug!("notification (sync done) failed: {e}");
    }
}

fn notify_sync_failed<R: Runtime>(app: &AppHandle<R>, project: &str, msg: &str) {
    use tauri_plugin_notification::NotificationExt;
    let res = app
        .notification()
        .builder()
        .title(format!("Sync failed: {project}"))
        .body(msg)
        .show();
    if let Err(e) = res {
        debug!("notification (sync failed) failed: {e}");
    }
}

/// Build the very first tray. Called once from `lib.rs::setup`. After
/// this, `rebuild_menu` mutates the existing tray rather than building a
/// new one.
pub fn build_initial_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let projects = app.state::<ProjectState>().snapshot();
    let menu = build_menu(app, &projects)?;

    // Initial icon: neutral (un-tinted) until the first poll cycle finishes.
    let icons = app.state::<TrayPollState>().icons.clone();
    let initial_icon = icons.neutral;

    let _tray = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("Spaceshop Companion")
        .icon(initial_icon)
        .on_menu_event(|app, event| {
            let id = event.id.as_ref();
            match id {
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
                        use tauri::Emitter;
                        let _ = win.emit("nav", "/onboard");
                    }
                }
                "quit" => {
                    // Tell the poll loop to bail before we tear down.
                    if let Some(state) = app.try_state::<TrayPollState>() {
                        state.shutdown.notify_waiters();
                    }
                    app.exit(0);
                }
                other => {
                    if let Some(project_id) = other.strip_prefix("sync-") {
                        trigger_tray_sync(app.clone(), project_id.to_string());
                    }
                }
            }
        })
        .on_tray_icon_event(|tray, event| {
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
