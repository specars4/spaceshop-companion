//! Invite-code parsing — mirrors `tools/perforce/invite.py` v=1.
//!
//! Schema is locked in `docs/INVITE_FORMAT.md` in the SPACESHOP TOOLS repo.
//! Companion's Rust validator must accept exactly what the Python builder
//! emits and reject everything else.

use base64::Engine;
use base64::engine::general_purpose;
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};

use super::errors::CompanionError;

pub const SCHEMA_VERSION: u32 = 1;
pub const URL_SCHEME: &str = "spaceshop-companion";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteData {
    pub v: u32,
    pub project_name: String,
    pub project_id: String,
    pub issued_at: String,
    pub expires_at: String,
    pub issued_by: String,
    pub perforce: PerforceSection,
    pub tailscale: TailscaleSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerforceSection {
    pub server: String,
    pub user: String,
    pub ticket: String,
    pub workspace_template: WorkspaceTemplate,
    pub local_root_default: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceTemplate {
    pub name: String,
    pub view: Vec<String>,
    pub options: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TailscaleSection {
    pub auth_key: String,
    pub tags: Vec<String>,
}

/// Strip whitespace, accept either bare code or `spaceshop-companion://invite/{code}`
/// URL form, restore base64 padding, decode → utf-8 JSON → validate.
pub fn parse_invite(raw_input: &str) -> Result<InviteData, CompanionError> {
    let code = extract_code(raw_input.trim());

    // urlsafe-b64-no-padding → pad back to multiple of 4
    let padded = pad_b64(code);
    let bytes = general_purpose::URL_SAFE
        .decode(padded.as_bytes())
        .map_err(|e| CompanionError::InviteInvalid(format!("not valid base64: {e}")))?;

    let json_text = std::str::from_utf8(&bytes)
        .map_err(|e| CompanionError::InviteInvalid(format!("not valid utf-8: {e}")))?;

    let payload: InviteData = serde_json::from_str(json_text)
        .map_err(|e| CompanionError::InviteInvalid(format!("not valid invite JSON: {e}")))?;

    validate(&payload)?;
    Ok(payload)
}

fn extract_code(s: &str) -> &str {
    if let Some(rest) = s.strip_prefix(&format!("{URL_SCHEME}://invite/")) {
        rest
    } else if let Some(rest) = s.strip_prefix(&format!("{URL_SCHEME}:/invite/")) {
        // Some platforms strip one slash from custom URL schemes.
        rest
    } else {
        s
    }
}

fn pad_b64(code: &str) -> String {
    let missing = (4 - code.len() % 4) % 4;
    let mut out = String::with_capacity(code.len() + missing);
    out.push_str(code);
    for _ in 0..missing {
        out.push('=');
    }
    out
}

fn validate(payload: &InviteData) -> Result<(), CompanionError> {
    if payload.v != SCHEMA_VERSION {
        return Err(CompanionError::InviteInvalid(format!(
            "unsupported invite version v={} (this build understands v={SCHEMA_VERSION})",
            payload.v
        )));
    }

    // Expiration: parsed as RFC 3339 (ISO 8601 with timezone).
    let exp = DateTime::parse_from_rfc3339(&payload.expires_at)
        .map_err(|e| CompanionError::InviteInvalid(format!("expires_at unparseable: {e}")))?
        .with_timezone(&Utc);
    if exp < Utc::now() {
        return Err(CompanionError::InviteExpired);
    }

    // Server: host:port shape.
    let server_re = Regex::new(r"^[A-Za-z0-9][A-Za-z0-9.\-]*:\d{1,5}$").unwrap();
    if !server_re.is_match(&payload.perforce.server) {
        return Err(CompanionError::InviteInvalid(format!(
            "perforce.server '{}' is not host:port",
            payload.perforce.server
        )));
    }

    // project_id: kebab-case ASCII.
    let pid_re = Regex::new(r"^[a-z0-9][a-z0-9\-]*$").unwrap();
    if !pid_re.is_match(&payload.project_id) {
        return Err(CompanionError::InviteInvalid(format!(
            "project_id '{}' must be kebab-case ASCII",
            payload.project_id
        )));
    }

    // Workspace view non-empty.
    if payload.perforce.workspace_template.view.is_empty() {
        return Err(CompanionError::InviteInvalid(
            "perforce.workspace_template.view must be non-empty".into(),
        ));
    }

    // Tag policy: must include tag:contractor for ACL enforcement on the tailnet.
    if !payload
        .tailscale
        .tags
        .iter()
        .any(|t| t == "tag:contractor")
    {
        return Err(CompanionError::InviteInvalid(
            "tailscale.tags must include 'tag:contractor'".into(),
        ));
    }

    Ok(())
}

#[tauri::command]
pub fn parse_invite_cmd(code: String) -> Result<InviteData, CompanionError> {
    parse_invite(&code)
}

/// Expand %USERPROFILE% and similar placeholders in a local-root string.
pub fn expand_local_root(template: &str) -> String {
    let mut out = template.to_string();
    for (var, val) in std::env::vars() {
        let placeholder = format!("%{}%", var);
        if out.contains(&placeholder) {
            out = out.replace(&placeholder, &val);
        }
    }
    // Normalize forward slashes too.
    out.replace('/', "\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    // The session-2 smoke invite (re-issued 2026-05-20 with reusable key).
    const SMOKE_INVITE: &str = "eyJ2IjoxLCJwcm9qZWN0X25hbWUiOiJTZXNzaW9uIDIgc21va2Ug4oCUIHNtb2tlLXY0IiwicHJvamVjdF9pZCI6InNlc3Npb24tMi1zbW9rZSIsImlzc3VlZF9hdCI6IjIwMjYtMDUtMjBUMDY6NTY6MjcrMDA6MDAiLCJleHBpcmVzX2F0IjoiMjAyNi0wOC0xOFQwNjo1NjoyNyswMDowMCIsImlzc3VlZF9ieSI6ImFyc2VuQHNwYWNlc2hvcC5zdHVkaW8iLCJwZXJmb3JjZSI6eyJzZXJ2ZXIiOiIxMDAuODIuMC44OjE2NjYiLCJ1c2VyIjoic2FyYWgtdGVzdCIsInRpY2tldCI6IjRDNTFEQjQ4RkIzOTE1RkI5RTE1NzIzRTIxNDg5ODg1Iiwid29ya3NwYWNlX3RlbXBsYXRlIjp7Im5hbWUiOiJzYXJhaC10ZXN0LXNlc3Npb24tMi1zbW9rZSIsInZpZXciOlsiLy9zbW9rZS12NC9tYWluLy4uLiAvL3NhcmFoLXRlc3Qtc2Vzc2lvbi0yLXNtb2tlLy4uLiJdLCJvcHRpb25zIjoibm9hbGx3cml0ZSBub2Nsb2JiZXIgbm9jb21wcmVzcyB1bmxvY2tlZCBub21vZHRpbWUgbm9ybWRpciJ9LCJsb2NhbF9yb290X2RlZmF1bHQiOiIlVVNFUlBST0ZJTEUlXFxTcGFjZXNob3BQcm9qZWN0c1xcc2Vzc2lvbi0yLXNtb2tlIn0sInRhaWxzY2FsZSI6eyJhdXRoX2tleSI6InRza2V5LWF1dGgta1lpQ1pCc1VCNzExQ05UUkwtTkpTYkRwaEw3NTRCc0hwd053QlM0NFVLaXYzVFhLV1dZIiwidGFncyI6WyJ0YWc6Y29udHJhY3RvciJdfX0";

    #[test]
    fn parses_smoke_invite() {
        let invite = parse_invite(SMOKE_INVITE).expect("smoke invite parses");
        assert_eq!(invite.v, 1);
        assert_eq!(invite.project_id, "session-2-smoke");
        assert_eq!(invite.perforce.server, "100.82.0.8:1666");
        assert_eq!(invite.perforce.user, "sarah-test");
        assert!(invite.tailscale.tags.contains(&"tag:contractor".to_string()));
    }

    #[test]
    fn parses_smoke_invite_url_form() {
        let url = format!("spaceshop-companion://invite/{}", SMOKE_INVITE);
        let invite = parse_invite(&url).expect("URL form parses");
        assert_eq!(invite.project_id, "session-2-smoke");
    }

    #[test]
    fn rejects_truncated_invite() {
        assert!(parse_invite(&SMOKE_INVITE[..40]).is_err());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_invite("hello world").is_err());
    }
}
