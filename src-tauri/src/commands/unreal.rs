//! Open a project's .uproject in Unreal Engine — with source control
//! pre-configured so Unreal connects to Perforce without the contractor
//! needing to type anything into Editor Preferences.
//!
//! Mechanism:
//!  1. Find the .uproject inside the workspace folder
//!  2. Pre-write `Saved/Config/WindowsEditor/SourceControlSettings.ini`
//!     with the project's server / user / workspace (Unreal reads this
//!     file on project load — UE 5.x convention; verified against the
//!     5.7 docs + community forum threads)
//!  3. Shell-launch the .uproject (`cmd /c start "" "<path>"`) so Windows
//!     file association opens it in whichever Unreal Editor is installed
//!
//! The contractor's p4 ticket is already at `%USERPROFILE%\p4tickets.txt`
//! (Companion populated it during onboarding), so Unreal authenticates
//! automatically once it knows the server/user/workspace from the INI.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};
use tracing::info;

use super::errors::CompanionError;
use super::projects::ProjectState;

const SKIP_DIRS: &[&str] = &[
    "Saved",
    "Intermediate",
    "DerivedDataCache",
    "Binaries",
    "Build",
    ".git",
    ".p4",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenInUnrealResult {
    pub uproject_path: String,
    pub config_path: String,
}

/// Find a `.uproject` inside `workspace_root`. Checks the root first, then
/// one level deep into subfolders that aren't engine cache dirs.
pub fn find_uproject(workspace_root: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(workspace_root).ok()?;
    let mut subdirs: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file()
            && p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("uproject"))
                .unwrap_or(false)
        {
            return Some(p);
        }
        if p.is_dir() {
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            if !SKIP_DIRS.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
                subdirs.push(p);
            }
        }
    }
    for dir in subdirs {
        if let Ok(inner) = std::fs::read_dir(&dir) {
            for entry in inner.flatten() {
                let p = entry.path();
                if p.is_file()
                    && p.extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.eq_ignore_ascii_case("uproject"))
                        .unwrap_or(false)
                {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Write `Saved/Config/WindowsEditor/SourceControlSettings.ini` next to
/// the .uproject so Unreal's Perforce source-control provider is
/// pre-configured.
pub fn write_source_control_settings(
    project_dir: &Path,
    server: &str,
    user: &str,
    workspace: &str,
) -> Result<PathBuf, CompanionError> {
    let dir = project_dir
        .join("Saved")
        .join("Config")
        .join("WindowsEditor");
    std::fs::create_dir_all(&dir).map_err(CompanionError::Io)?;
    let path = dir.join("SourceControlSettings.ini");

    let body = format!(
        "[SourceControl.SourceControlSettings]\n\
         Provider=Perforce\n\
         \n\
         [PerforceSourceControl.PerforceSourceControlSettings]\n\
         Port={server}\n\
         UserName={user}\n\
         Workspace={workspace}\n\
         HostOverride=\n\
         UseP4Config=False\n"
    );

    // Clear read-only bit if Unreal had previously written this file and
    // marked it readonly (rare but possible after a `p4 sync` if it's
    // versioned — which it shouldn't be).
    if path.exists() {
        if let Ok(meta) = std::fs::metadata(&path) {
            let mut perms = meta.permissions();
            if perms.readonly() {
                perms.set_readonly(false);
                let _ = std::fs::set_permissions(&path, perms);
            }
        }
    }

    std::fs::write(&path, body).map_err(CompanionError::Io)?;
    Ok(path)
}

#[tauri::command]
pub async fn open_in_unreal<R: Runtime>(
    app: AppHandle<R>,
    project_id: String,
) -> Result<OpenInUnrealResult, CompanionError> {
    let project = {
        let state = app.state::<ProjectState>();
        let projects = state.snapshot();
        projects
            .iter()
            .find(|p| p.project_id == project_id)
            .cloned()
    };
    let Some(project) = project else {
        return Err(CompanionError::Other(format!(
            "no project {project_id}"
        )));
    };

    let workspace_root = PathBuf::from(&project.workspace_root);
    let Some(uproject) = find_uproject(&workspace_root) else {
        return Err(CompanionError::Other(format!(
            "No .uproject file found in {}. Run Pull latest to download the project's content first.",
            workspace_root.display()
        )));
    };

    let project_dir = uproject.parent().unwrap_or(&workspace_root);
    let config_path = write_source_control_settings(
        project_dir,
        &project.server,
        &project.user,
        &project.workspace_name,
    )?;

    info!(
        "launching {} (config at {})",
        uproject.display(),
        config_path.display()
    );

    // cmd /c start "" "<path>" — uses Windows file association.
    // The empty quoted string is the window title (required by `start`
    // when the first arg has spaces, otherwise it gets misinterpreted).
    let uproject_str = uproject.to_string_lossy().to_string();
    StdCommand::new("cmd")
        .args(["/c", "start", "", &uproject_str])
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|e| {
            CompanionError::Other(format!("could not launch Unreal: {e}"))
        })?;

    Ok(OpenInUnrealResult {
        uproject_path: uproject_str,
        config_path: config_path.to_string_lossy().to_string(),
    })
}
