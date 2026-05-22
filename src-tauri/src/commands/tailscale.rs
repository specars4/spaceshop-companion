//! Tailscale lifecycle — bundle, install service, up/status/down.
//!
//! Tailscale on Windows runs as a SYSTEM service (`Tailscale`). The
//! service binary is `tailscaled.exe`, the CLI is `tailscale.exe`.
//! Installing the service requires elevation; subsequent calls
//! (`tailscale up --auth-key=…`) talk to the running service via a
//! local IPC socket and do NOT require elevation.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tokio::time::timeout;

use super::errors::CompanionError;
use super::paths;

const CALL_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TailscaleStatus {
    pub up: bool,
    pub backend_state: String,
    pub self_ip: Option<String>,
    pub tailnet_name: Option<String>,
}

fn no_window() -> u32 {
    // CREATE_NO_WINDOW — hides the child console window on Windows.
    0x08000000
}

async fn run_tailscale(
    exe: &PathBuf,
    args: &[&str],
) -> Result<(String, String, i32), CompanionError> {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(no_window());

    let child = cmd
        .spawn()
        .map_err(|e| CompanionError::Tailscale(format!("could not spawn tailscale: {e}")))?;

    let output = timeout(CALL_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| CompanionError::Tailscale("tailscale command timed out".into()))?
        .map_err(|e| CompanionError::Tailscale(format!("tailscale wait failed: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);
    Ok((stdout, stderr, code))
}

/// `tailscale up --auth-key=<key> --reset --hostname=<host> --accept-routes`
///
/// `--reset` is intentional — fresh contractor installs should start
/// from a clean state regardless of prior config. `--accept-routes` lets
/// the contractor reach the NAS's subnet-routed ranges if Arsen later
/// enables them.
pub async fn up<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    auth_key: &str,
    hostname: &str,
) -> Result<TailscaleStatus, CompanionError> {
    let exe = PathBuf::from(paths::tailscale_exe(app).map_err(CompanionError::Tailscale)?);
    if !exe.exists() {
        return Err(CompanionError::Tailscale(format!(
            "tailscale.exe missing from bundled binaries: {}",
            exe.display()
        )));
    }

    let args = vec![
        "up",
        "--auth-key",
        auth_key,
        "--reset",
        "--hostname",
        hostname,
        "--accept-routes",
        "--timeout",
        "30s",
    ];
    let (stdout, stderr, code) = run_tailscale(&exe, &args).await?;
    if code != 0 {
        let msg = if !stderr.trim().is_empty() {
            stderr
        } else {
            stdout
        };
        return Err(CompanionError::Tailscale(msg.trim().into()));
    }
    status(app).await
}

/// `tailscale status --json` → parse backend state + assigned IP.
pub async fn status<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<TailscaleStatus, CompanionError> {
    let exe = PathBuf::from(paths::tailscale_exe(app).map_err(CompanionError::Tailscale)?);
    if !exe.exists() {
        return Ok(TailscaleStatus {
            up: false,
            backend_state: "NotInstalled".into(),
            self_ip: None,
            tailnet_name: None,
        });
    }

    let (stdout, stderr, code) = run_tailscale(&exe, &["status", "--json"]).await?;
    if code != 0 {
        // If the service is down, `tailscale status` exits non-zero with a
        // recognisable message; we surface that as a structured Down status
        // rather than an error.
        let combined = format!("{stdout}\n{stderr}").to_lowercase();
        if combined.contains("tailscaled is not running")
            || combined.contains("service has not been started")
            || combined.contains("connection refused")
        {
            return Ok(TailscaleStatus {
                up: false,
                backend_state: "ServiceDown".into(),
                self_ip: None,
                tailnet_name: None,
            });
        }
        return Err(CompanionError::Tailscale(if !stderr.trim().is_empty() {
            stderr
        } else {
            stdout
        }));
    }

    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| CompanionError::Tailscale(format!("status JSON unparseable: {e}")))?;

    let backend_state = parsed
        .get("BackendState")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();

    let self_ip = parsed
        .get("Self")
        .and_then(|s| s.get("TailscaleIPs"))
        .and_then(|ips| ips.as_array())
        .and_then(|arr| arr.iter().filter_map(|v| v.as_str()).next())
        .map(|s| s.to_string());

    let tailnet_name = parsed
        .get("MagicDNSSuffix")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok(TailscaleStatus {
        up: backend_state == "Running",
        backend_state,
        self_ip,
        tailnet_name,
    })
}

pub async fn down<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<(), CompanionError> {
    let exe = PathBuf::from(paths::tailscale_exe(app).map_err(CompanionError::Tailscale)?);
    let (_, stderr, code) = run_tailscale(&exe, &["down"]).await?;
    if code != 0 {
        return Err(CompanionError::Tailscale(stderr.trim().into()));
    }
    Ok(())
}

/// Install Tailscale on Windows by running the bundled MSI silently.
/// Requires admin elevation — will trigger a UAC prompt (the bundled MSI
/// internally registers the `Tailscale` service which needs SYSTEM
/// install rights).
///
/// We launch `msiexec` via PowerShell's `Start-Process -Verb RunAs -Wait`
/// so the UAC prompt fires correctly and we can wait for completion
/// before probing.
pub async fn install_via_msi<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), CompanionError> {
    let msi = PathBuf::from(
        paths::tailscale_msi_resource(app).map_err(CompanionError::Tailscale)?,
    );
    if !msi.exists() {
        return Err(CompanionError::Tailscale(format!(
            "bundled tailscale.msi missing: {}",
            msi.display()
        )));
    }

    // /qn → fully silent. TS_UNATTENDEDMODE prevents Tailscale from popping
    // its own first-run UI. We do NOT pass an --auth-key here — that goes
    // to `tailscale up` in the next step so failures surface separately.
    let ps_command = format!(
        "Start-Process -FilePath 'msiexec.exe' -ArgumentList '/i','\"{}\"','/qn','/norestart','TS_UNATTENDEDMODE=always' -Verb RunAs -Wait -PassThru | Select-Object -ExpandProperty ExitCode",
        msi.display()
    );

    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &ps_command])
        .creation_flags(no_window())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = cmd
        .spawn()
        .map_err(|e| CompanionError::Tailscale(format!("could not spawn msiexec: {e}")))?;
    let output = timeout(Duration::from_secs(300), child.wait_with_output())
        .await
        .map_err(|_| CompanionError::Tailscale("msi install timed out (>5 min)".into()))?
        .map_err(|e| CompanionError::Tailscale(format!("msi install wait failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(CompanionError::Tailscale(format!(
            "msi install failed: {}",
            stderr.trim()
        )));
    }
    // PowerShell's Select-Object prints the exit code of msiexec. 0 = success,
    // 3010 = success-but-reboot-required, 1602 = user-cancelled UAC.
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let exit: i32 = stdout.parse().unwrap_or(0);
    match exit {
        0 | 3010 => Ok(()),
        1602 | 1603 => Err(CompanionError::Tailscale(
            "Permission denied. Click Yes on the Windows permission prompt next time.".into(),
        )),
        other => Err(CompanionError::Tailscale(format!(
            "Tailscale installer failed (exit {other})"
        ))),
    }
}

// ----- Tauri commands -----

#[tauri::command]
pub async fn tailscale_status<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<TailscaleStatus, CompanionError> {
    status(&app).await
}

#[tauri::command]
pub async fn tailscale_up<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    auth_key: String,
    hostname: String,
) -> Result<TailscaleStatus, CompanionError> {
    up(&app, &auth_key, &hostname).await
}

#[tauri::command]
pub async fn tailscale_install<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<(), CompanionError> {
    install_via_msi(&app).await
}
