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
        // Defense-in-depth: tailscale.exe today doesn't echo the auth
        // key back in error messages, but if it ever does, we don't
        // want a still-usable single-use key flowing into our friendly
        // error → React UI → console.log → screenshot pipeline. Scrub
        // it from both streams before constructing the error.
        let stderr = stderr.replace(auth_key, "<redacted-auth-key>");
        let stdout = stdout.replace(auth_key, "<redacted-auth-key>");
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

    // Defensive parse: tailscale.exe occasionally emits partial JSON while
    // the service is still warming up after install. Treat unparseable
    // output as "Unknown / not up" rather than bubbling an Err that
    // upstream callers would interpret as "couldn't determine state →
    // skip install". A genuine "tailscale.exe missing" case was already
    // handled at the top of this function via the exe.exists() check,
    // so a parse failure here genuinely is "tailscaled is alive but
    // not yet emitting valid status JSON."
    let parsed: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                "tailscale status returned unparseable JSON ({} bytes): {e}",
                stdout.len()
            );
            return Ok(TailscaleStatus {
                up: false,
                backend_state: "Unknown".into(),
                self_ip: None,
                tailnet_name: None,
            });
        }
    };

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
    let src_msi = PathBuf::from(
        paths::tailscale_msi_resource(app).map_err(CompanionError::Tailscale)?,
    );
    if !src_msi.exists() {
        return Err(CompanionError::Tailscale(format!(
            "bundled tailscale.msi missing at expected path: {}",
            src_msi.display()
        )));
    }
    let src_size = std::fs::metadata(&src_msi).map(|m| m.len()).unwrap_or(0);

    // The bundled MSI lives at C:\Program Files\Spaceshop Companion\resources\
    // binaries\tailscale.msi — TWO spaces in the path. msiexec invoked via
    // elevated PowerShell can choke on quoted spacey paths with exit 1619
    // (ERROR_INSTALL_PACKAGE_OPEN_FAILED) on some Windows configs.
    //
    // Workaround: copy the MSI to a no-spaces temp path before handing it
    // to msiexec. The temp dir is readable by anyone (including elevated
    // SYSTEM context) and never has spaces under any normal Windows setup.
    let temp_dir = std::env::temp_dir();
    // Include the current process id in the temp filename so two users
    // running install simultaneously on a shared box (or two failed runs
    // overlapping) don't trample each other's MSI copies. Each process
    // owns its own file from copy through cleanup.
    let dst_msi = temp_dir.join(format!(
        "spaceshop-companion-tailscale-{}.msi",
        std::process::id()
    ));
    // Always overwrite — if a previous failed run left a partial file behind
    // it could itself trigger 1619.
    std::fs::copy(&src_msi, &dst_msi).map_err(|e| {
        CompanionError::Tailscale(format!(
            "could not copy tailscale.msi to temp ({} → {}): {e}",
            src_msi.display(),
            dst_msi.display()
        ))
    })?;
    let dst_size = std::fs::metadata(&dst_msi).map(|m| m.len()).unwrap_or(0);
    if dst_size != src_size {
        return Err(CompanionError::Tailscale(format!(
            "tailscale.msi temp copy is the wrong size (src={} bytes, dst={} bytes)",
            src_size, dst_size
        )));
    }

    // If the user's profile happens to contain a space (rare but real),
    // even the temp path can have spaces and re-trigger the same 1619.
    // Convert to the 8.3 short-name form via Win32 GetShortPathName — those
    // are guaranteed space-free. Falls back to the long path if the
    // conversion fails for any reason.
    let msi_path_for_msiexec = to_short_path_if_possible(&dst_msi);

    // /qn → fully silent. TS_UNATTENDEDMODE prevents Tailscale from popping
    // its own first-run UI. We do NOT pass an --auth-key here — that goes
    // to `tailscale up` in the next step so failures surface separately.
    //
    // We do NOT use `-Wait` on Start-Process here. When combined with
    // `-Verb RunAs`, `-Wait` races with the elevated child: the cmdlet
    // can return before the elevated process has fully terminated, and
    // any subsequent attempt to read `.ExitCode` blows up with
    //   "Process must exit before requested information is available."
    // Instead we capture the Process object with `-PassThru` and call
    // `.WaitForExit()` explicitly, which is guaranteed to block until
    // the process has actually exited and the exit code is readable.
    let ps_command = format!(
        "$p = Start-Process -FilePath 'msiexec.exe' -ArgumentList '/i','\"{}\"','/qn','/norestart','TS_UNATTENDEDMODE=always' -Verb RunAs -PassThru; $p.WaitForExit(); $p.ExitCode",
        msi_path_for_msiexec.display()
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

    // Best-effort cleanup of the temp copy regardless of outcome.
    let _ = std::fs::remove_file(&dst_msi);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(CompanionError::Tailscale(format!(
            "powershell failed launching msiexec (stderr: {}, src={}, dst={}, size={} bytes)",
            stderr.trim(),
            src_msi.display(),
            dst_msi.display(),
            src_size,
        )));
    }
    // PowerShell prints msiexec's ExitCode. 0 = success, 3010 = reboot required,
    // 1602/1603 = user cancelled UAC, 1619 = could not open package, etc.
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let exit: i32 = stdout.parse().unwrap_or(0);
    match exit {
        0 | 3010 => Ok(()),
        1602 | 1603 => Err(CompanionError::Tailscale(
            "Permission denied. Click Yes on the Windows permission prompt next time.".into(),
        )),
        1619 => Err(CompanionError::Tailscale(format!(
            "Tailscale installer could not be opened (exit 1619). \
             Tried path: {}. Source: {} ({} bytes). \
             As a workaround, install Tailscale manually from https://tailscale.com/download/windows then click Try Again.",
            dst_msi.display(),
            src_msi.display(),
            src_size,
        ))),
        other => Err(CompanionError::Tailscale(format!(
            "Tailscale installer failed (exit {other}). Source: {} ({} bytes). Temp copy: {}.",
            src_msi.display(),
            src_size,
            dst_msi.display(),
        ))),
    }
}

/// Convert a path to its Windows 8.3 short-name form if possible. The
/// short-name version is guaranteed to contain no spaces, which dodges
/// the family of msiexec/Start-Process argument-quoting bugs that
/// produce exit 1619 (ERROR_INSTALL_PACKAGE_OPEN_FAILED) when paths
/// contain spaces. Requires the file to actually exist at the input
/// path. Falls back to returning the input path unchanged if the
/// Win32 call fails for any reason (e.g., the file system has 8.3
/// generation disabled, which is uncommon).
fn to_short_path_if_possible(path: &std::path::Path) -> std::path::PathBuf {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetShortPathNameW;

    let wide: Vec<u16> = path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut buf = [0u16; 1024];
    // SAFETY: wide is null-terminated, buf is correctly sized, and the
    // Win32 API is documented to return either 0 (failure) or the
    // number of chars excluding the null terminator.
    let ret = unsafe { GetShortPathNameW(PCWSTR(wide.as_ptr()), Some(&mut buf)) };
    if ret > 0 && (ret as usize) < buf.len() {
        let s = String::from_utf16_lossy(&buf[..ret as usize]);
        return std::path::PathBuf::from(s);
    }
    path.to_path_buf()
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
