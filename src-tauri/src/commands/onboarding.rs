//! The full bootstrap chain — what runs when a contractor pastes an
//! invite and clicks "Connect".
//!
//! Steps, each emitting a `onboarding-step` Tauri event the UI listens
//! for so progress is visible:
//!   1. parse + validate invite
//!   2. (if not running) install + start the tailscale service [needs UAC]
//!   3. `tailscale up --auth-key=...` to join the tailnet
//!   4. probe `p4 info` to confirm the server is reachable + ticket good
//!   5. write %USERPROFILE%\p4tickets.txt so Unreal finds the ticket
//!   6. create the workspace (Unreal-friendly options)
//!   7. initial `p4 sync //...` into the contractor-chosen folder
//!   8. persist the project record + return it to the UI
//!
//! On failure at any step the error is wrapped in `FriendlyError` and the
//! UI shows it next to the step that failed.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, Runtime};

use super::errors::CompanionError;
use super::invite::{InviteData, parse_invite};
use super::paths;
use super::perforce::{self, P4Config};
use super::projects::{save_persisted, Project, ProjectState};
use super::tailscale;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum OnboardingEvent {
    Step { step: String, status: String, detail: Option<String> },
    SyncProgress { line: String, files_so_far: u32 },
    Complete { project: Project },
    Failed { step: String, error: super::errors::FriendlyError },
}

const EVENT_NAME: &str = "onboarding-event";

fn emit<R: Runtime>(app: &tauri::AppHandle<R>, event: OnboardingEvent) {
    let _ = app.emit(EVENT_NAME, event);
}

fn step<R: Runtime>(app: &tauri::AppHandle<R>, step: &str, status: &str, detail: Option<String>) {
    emit(
        app,
        OnboardingEvent::Step {
            step: step.into(),
            status: status.into(),
            detail,
        },
    );
}

fn fail<R: Runtime>(app: &tauri::AppHandle<R>, step: &str, err: &CompanionError) {
    emit(
        app,
        OnboardingEvent::Failed {
            step: step.into(),
            error: err.to_friendly(),
        },
    );
}

#[tauri::command]
pub async fn apply_invite<R: Runtime>(
    app: tauri::AppHandle<R>,
    code: String,
    workspace_root: String,
) -> Result<Project, CompanionError> {
    // ----- 1. Parse invite -----
    step(&app, "parse", "running", None);
    let invite: InviteData = match parse_invite(&code) {
        Ok(i) => i,
        Err(e) => {
            fail(&app, "parse", &e);
            return Err(e);
        }
    };
    step(
        &app,
        "parse",
        "done",
        Some(format!("Project: {}", invite.project_name)),
    );

    // ----- 2. Ensure tailscale service is up -----
    step(&app, "service", "running", None);
    let ts_status = tailscale::status(&app).await.unwrap_or_else(|_| {
        // Treat probe failure as "we don't know" → try install
        super::tailscale::TailscaleStatus {
            up: false,
            backend_state: "Unknown".into(),
            self_ip: None,
            tailnet_name: None,
        }
    });

    if ts_status.backend_state == "ServiceDown" || ts_status.backend_state == "NotInstalled" {
        step(
            &app,
            "service",
            "installing",
            Some("Asking Windows for permission…".into()),
        );
        if let Err(e) = tailscale::install_via_msi(&app).await {
            fail(&app, "service", &e);
            return Err(e);
        }
    }
    step(&app, "service", "done", None);

    // ----- 3. Tailscale up -----
    step(&app, "connect", "running", None);
    let hostname = format!(
        "spaceshop-{}",
        hostname_safe(&invite.perforce.user)
    );
    let ts = match tailscale::up(&app, &invite.tailscale.auth_key, &hostname).await {
        Ok(s) => s,
        Err(e) => {
            fail(&app, "connect", &e);
            return Err(e);
        }
    };
    step(
        &app,
        "connect",
        "done",
        ts.self_ip.clone().map(|ip| format!("Joined as {ip}")),
    );

    // ----- 4. Probe the project server -----
    step(&app, "server", "running", None);
    let p4_exe: PathBuf = PathBuf::from(paths::p4_exe(&app).map_err(CompanionError::Perforce)?);
    let cfg = P4Config {
        server: invite.perforce.server.clone(),
        user: invite.perforce.user.clone(),
        ticket: invite.perforce.ticket.clone(),
        p4_exe,
    };
    let info = match perforce::info(&cfg).await {
        Ok(i) => i,
        Err(e) => {
            fail(&app, "server", &e);
            return Err(e);
        }
    };
    step(
        &app,
        "server",
        "done",
        Some(format!("Server {} ready", info.server_version)),
    );

    // ----- 5. Write ticket so Unreal finds it -----
    step(&app, "ticket", "running", None);
    if let Err(e) =
        perforce::write_ticket_file(&invite.perforce.server, &invite.perforce.user, &invite.perforce.ticket)
    {
        fail(&app, "ticket", &e);
        return Err(e);
    }
    step(&app, "ticket", "done", None);

    // ----- 6. Create workspace -----
    step(&app, "workspace", "running", None);
    let root = Path::new(&workspace_root);
    if let Err(e) = perforce::create_workspace(&cfg, &invite.perforce.workspace_template, root).await
    {
        fail(&app, "workspace", &e);
        return Err(e);
    }
    step(
        &app,
        "workspace",
        "done",
        Some(invite.perforce.workspace_template.name.clone()),
    );

    // ----- 7. Sync — adopt-in-place OR fresh download -----
    //
    // Companion v0.6.2: an invite can carry `adopt_existing: true`
    // (Workshop's SELF-ONBOARD INVITE button) which tells us the
    // workspace files are ALREADY on disk at `workspace_root`. In that
    // case we MUST NOT run initial_sync — it flushes the have-table
    // and re-downloads everything, which for a 50GB Unreal project at
    // 100 Mbps means an hour-plus of needless transfer.
    //
    // Decision matrix:
    //   adopt_existing == false  → initial_sync (fresh download — the
    //                              contractor-onboard default that
    //                              predates v0.6.2)
    //   adopt_existing == true, root empty → fall back to initial_sync
    //     (the invite said adopt but the folder is bare — operator
    //     likely picked a different folder in CHOOSE FOLDER)
    //   adopt_existing == true, root has files → adopt_in_place
    //     (sync -k + reconcile -n, ~30s, no file transfer)
    step(&app, "sync", "running", None);
    let app_for_progress = app.clone();
    let adopt_existing = invite.adopt_existing.unwrap_or(false);
    let root_has_content = std::fs::read_dir(root)
        .map(|mut it| it.next().is_some())
        .unwrap_or(false);

    let (files, sync_detail) = if adopt_existing && root_has_content {
        // Adopt-in-place — have-table update + reconcile audit, no transfer.
        let app_for_adopt = app_for_progress.clone();
        match perforce::adopt_in_place(
            &cfg,
            &invite.perforce.workspace_template.name,
            root,
            invite.adopt_at_cl,
            |line| {
                let _ = app_for_adopt.emit(
                    EVENT_NAME,
                    OnboardingEvent::SyncProgress {
                        line: line.into(),
                        files_so_far: 0,
                    },
                );
            },
        )
        .await
        {
            Ok(outcome) => {
                let detail = if outcome.reconcile_diff_count == 0 {
                    format!(
                        "Adopted {} files — workspace matches depot",
                        outcome.have_table_files
                    )
                } else {
                    format!(
                        "Adopted {} files — {} pending changes detected (review in Companion)",
                        outcome.have_table_files, outcome.reconcile_diff_count
                    )
                };
                (outcome.have_table_files, detail)
            }
            Err(e) => {
                fail(&app, "sync", &e);
                return Err(e);
            }
        }
    } else {
        // Fresh download — contractor onboard, or self-onboard fallback
        // when the chosen folder is empty.
        if adopt_existing && !root_has_content {
            // Soft-notify operator: we ignored the adopt flag because
            // the folder is bare. UI step text reflects this.
            let _ = app_for_progress.emit(
                EVENT_NAME,
                OnboardingEvent::SyncProgress {
                    line: "(adopt flag ignored — chosen folder is empty; running fresh sync)"
                        .into(),
                    files_so_far: 0,
                },
            );
        }
        match perforce::initial_sync(
            &cfg,
            &invite.perforce.workspace_template.name,
            root,
            |line| {
                let _ = app_for_progress.emit(
                    EVENT_NAME,
                    OnboardingEvent::SyncProgress {
                        line: line.into(),
                        files_so_far: 0,
                    },
                );
            },
        )
        .await
        {
            Ok(n) => (n, format!("Pulled {} files", n)),
            Err(e) => {
                fail(&app, "sync", &e);
                return Err(e);
            }
        }
    };
    step(&app, "sync", "done", Some(sync_detail));

    // ----- 8. Persist project record -----
    let mut project = Project::from_invite(&invite, workspace_root.clone());
    project.last_sync_at = Some(chrono::Utc::now().to_rfc3339());
    project.last_sync_files = Some(files);

    let state = app.state::<ProjectState>();
    {
        let mut guard = state
            .inner
            .lock()
            .map_err(|_| CompanionError::Other("state poisoned".into()))?;
        // Replace any prior entry for this project_id.
        guard.retain(|p| p.project_id != project.project_id);
        guard.push(project.clone());
        let snapshot = guard.clone();
        drop(guard);
        save_persisted(&app, &snapshot).map_err(CompanionError::Other)?;
    }

    // Refresh tray menu so the new project gets its "Sync <Name>" item.
    super::tray_poll::rebuild_menu(&app);

    emit(
        &app,
        OnboardingEvent::Complete {
            project: project.clone(),
        },
    );
    Ok(project)
}

fn hostname_safe(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect()
}
