//! Persisted project state.
//!
//! One entry per project the contractor has onboarded. Stored via
//! `tauri-plugin-store` as JSON at `%APPDATA%\Spaceshop\Companion\projects.json`.
//!
//! The ticket inside `perforce.ticket` is sensitive but we persist it in
//! plaintext for v0.5 — the same file is also written to
//! `%USERPROFILE%\p4tickets.txt` by Perforce itself, so an attacker with
//! local-disk access already has it. v0.6 will DPAPI-wrap both.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{Manager, Runtime};
use tauri_plugin_store::StoreExt;

use super::errors::CompanionError;
use super::invite::InviteData;

const STORE_FILE: &str = "projects.json";
const KEY_PROJECTS: &str = "projects";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub project_id: String,
    pub project_name: String,
    pub issued_by: String,
    pub expires_at: String,
    pub workspace_name: String,
    pub workspace_root: String,
    pub server: String,
    pub user: String,
    pub ticket: String,
    pub last_sync_at: Option<String>,
    pub last_sync_files: Option<u32>,
}

impl Project {
    pub fn from_invite(invite: &InviteData, workspace_root: String) -> Self {
        Self {
            project_id: invite.project_id.clone(),
            project_name: invite.project_name.clone(),
            issued_by: invite.issued_by.clone(),
            expires_at: invite.expires_at.clone(),
            workspace_name: invite.perforce.workspace_template.name.clone(),
            workspace_root,
            server: invite.perforce.server.clone(),
            user: invite.perforce.user.clone(),
            ticket: invite.perforce.ticket.clone(),
            last_sync_at: None,
            last_sync_files: None,
        }
    }
}

#[derive(Default)]
pub struct ProjectState {
    pub inner: Mutex<Vec<Project>>,
}

impl ProjectState {
    pub fn replace_all(&self, projects: Vec<Project>) {
        if let Ok(mut g) = self.inner.lock() {
            *g = projects;
        }
    }

    pub fn snapshot(&self) -> Vec<Project> {
        self.inner.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

pub fn load_persisted<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<Vec<Project>, String> {
    let store = app
        .store(STORE_FILE)
        .map_err(|e| format!("could not open store: {e}"))?;
    let val = store.get(KEY_PROJECTS).unwrap_or(serde_json::Value::Null);
    if val.is_null() {
        return Ok(Vec::new());
    }
    serde_json::from_value(val).map_err(|e| format!("project store malformed: {e}"))
}

pub fn save_persisted<R: Runtime>(
    app: &tauri::AppHandle<R>,
    projects: &[Project],
) -> Result<(), String> {
    let store = app
        .store(STORE_FILE)
        .map_err(|e| format!("could not open store: {e}"))?;
    store.set(
        KEY_PROJECTS,
        serde_json::to_value(projects).map_err(|e| format!("serialize failed: {e}"))?,
    );
    store
        .save()
        .map_err(|e| format!("could not save store: {e}"))?;
    Ok(())
}

// ----- Tauri commands -----

#[tauri::command]
pub async fn list_projects<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Vec<Project>, CompanionError> {
    let state = app.state::<ProjectState>();
    let guard = state.inner.lock().map_err(|_| CompanionError::Other("state poisoned".into()))?;
    Ok(guard.clone())
}

#[tauri::command]
pub async fn remove_project<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_id: String,
) -> Result<(), CompanionError> {
    let state = app.state::<ProjectState>();
    {
        let mut guard = state
            .inner
            .lock()
            .map_err(|_| CompanionError::Other("state poisoned".into()))?;
        guard.retain(|p| p.project_id != project_id);
        let snapshot = guard.clone();
        drop(guard);
        save_persisted(&app, &snapshot).map_err(CompanionError::Other)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn open_project_folder<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_id: String,
) -> Result<(), CompanionError> {
    let state = app.state::<ProjectState>();
    let root = {
        let guard = state
            .inner
            .lock()
            .map_err(|_| CompanionError::Other("state poisoned".into()))?;
        guard
            .iter()
            .find(|p| p.project_id == project_id)
            .map(|p| p.workspace_root.clone())
    };
    let Some(root) = root else {
        return Err(CompanionError::Other(format!(
            "no project with id {project_id}"
        )));
    };
    open_folder_in_explorer(&root)
}

fn open_folder_in_explorer(path: &str) -> Result<(), CompanionError> {
    use std::os::windows::process::CommandExt;
    use std::process::Command as StdCommand;

    // Same validation as p4_reveal_in_explorer: reject leading slash,
    // require absolute path, canonicalize so ".." segments can't be
    // smuggled. See perforce::validate_explorer_path for rationale.
    let canonical = super::perforce::validate_explorer_path(path)?;
    let canonical_str = canonical.to_string_lossy();
    let cleaned = canonical_str
        .strip_prefix(r"\\?\")
        .unwrap_or(&canonical_str)
        .to_string();

    StdCommand::new("explorer.exe")
        .arg(&cleaned)
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|e| CompanionError::Other(format!("could not open explorer: {e}")))?;
    Ok(())
}
