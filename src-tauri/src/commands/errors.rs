//! Friendly error layer.
//!
//! Companion translates raw p4 / tailscale / IO error text into structured
//! plain-language messages the UI can show without exposing jargon
//! ("Perforce", "tailnet", "depot", "p4d", "workspace").

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
#[allow(dead_code)] // Connection/Auth currently funnel through translate(); kept for direct use.
pub enum CompanionError {
    #[error("invite is not valid: {0}")]
    InviteInvalid(String),

    #[error("invite has expired")]
    InviteExpired,

    #[error("connection problem: {0}")]
    Connection(String),

    #[error("auth problem: {0}")]
    Auth(String),

    #[error("tailscale problem: {0}")]
    Tailscale(String),

    #[error("p4 problem: {0}")]
    Perforce(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("other: {0}")]
    Other(String),
}

impl From<anyhow::Error> for CompanionError {
    fn from(e: anyhow::Error) -> Self {
        CompanionError::Other(format!("{:#}", e))
    }
}

/// Serializable shape the frontend reads. Title is a 4-7 word headline,
/// body is one sentence the contractor can understand, suggested_action
/// is what they should try next (or who to contact).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriendlyError {
    pub title: String,
    pub body: String,
    pub suggested_action: String,
    /// Raw error text — surfaced only in a "Details" expander, never in the
    /// primary message. Useful when the operator forwards a bug report.
    pub details: String,
}

impl FriendlyError {
    fn new(
        title: impl Into<String>,
        body: impl Into<String>,
        action: impl Into<String>,
        details: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            suggested_action: action.into(),
            details: details.into(),
        }
    }
}

// Canonical auth-error phrase set — MUST match
// `tools/perforce/p4_client.py::_AUTH_ERROR_PHRASES` in the SPACESHOP
// TOOLS repo. Any phrase that classifies as P4AuthError on the
// Workshop side must classify the same way here, or a real p4d auth
// message produces "ask for a new invite" guidance in Companion while
// Workshop silently auto-relogs in. Drift cost: contractor wasted
// round-trip discovering the mismatch.
//
// The previous catch-all `"perforce password"` was dropped because it
// over-matched (e.g. would catch "Perforce password reset
// successfully" — a benign info message). Phrases now mirror the
// SPACESHOP TOOLS canonical list verbatim, lowercase.
const AUTH_PHRASES: &[&str] = &[
    "perforce password (p4passwd) invalid or unset",
    "your session has expired",
    "user without password",
    "password not set",
    "must be reset",
];

const CONNECT_PHRASES: &[&str] = &[
    "connect to server failed",
    "tcp connect to ",
    "unable to connect to",
    "no route to host",
    "connection refused",
    "connection timed out",
    "name resolution failed",
];

const TAILSCALE_DOWN: &[&str] = &[
    "tailscaled is not running",
    "could not connect to tailscale",
    "service not running",
    "the service has not been started",
    "access is denied",
];

const TAILSCALE_KEY_USED: &[&str] = &[
    "auth key already used",
    "key already used",
    "ephemeral key",
    "not authorized",
    "invalid key",
];

/// True "couldn't reach the update server" — DNS, TCP connect, timeout,
/// refused connection, TLS handshake failure. The fix is "check your
/// internet." Distinct from UPDATE_VERIFY_PHRASES below because the
/// remediation advice differs.
const UPDATE_NETWORK_PHRASES: &[&str] = &[
    "could not reach",
    "could not connect",
    "connection refused",
    "connection timed out",
    "connection reset",
    "dns",
    "name resolution",
    "tls handshake",
    "network is unreachable",
    "no route to host",
    // v0.6.6 audit — bare `"timed out"` REMOVED from this list.
    // It over-matched on Perforce timeouts (`"p4 -c timed out after
    // 30.0s"`) misclassifying them as "Couldn't reach update server."
    // Specific cases that ARE network timeouts already match via
    // "connection timed out" above.
];

/// "We reached the server but couldn't trust / parse what came back."
/// Includes signature mismatches (minisign), malformed JSON, schema
/// errors, missing fields. The fix is usually "your installer's
/// signing key is older than the server's release manifest — you need
/// a fresher installer from the project lead." Configuration-placeholder
/// errors land here too — same UX (user can't act except by reinstalling
/// or asking for a new build).
const UPDATE_VERIFY_PHRASES: &[&str] = &[
    "could not fetch a valid release json",
    "signature mismatch",
    "minisign",
    "signature verification",
    "could not verify",
    "release json",
    "endpoint not configured",
    "placeholder in tauri.conf.json",
    "update endpoint",
];

/// Generic update-check failure phrases that don't fit either bucket.
/// We classify these as verify-ish (treat as benign, don't blame the
/// network) because they typically come from no-update-available or
/// "no release found" cases that aren't actionable for the user.
const UPDATE_GENERIC_PHRASES: &[&str] = &[
    "update check failed",
    "no update available",
];

const STREAM_CLIENT_PHRASES: &[&str] = &[
    "cannot submit from non-stream client",
    "non-stream client",
];

pub fn translate(raw: &str) -> FriendlyError {
    let lc = raw.to_lowercase();

    // Check verify-class FIRST: a "signature mismatch" message often also
    // mentions "could not" / similar substrings that would match
    // network-class. Verify failures are more specific so they win the
    // classification race.
    if UPDATE_VERIFY_PHRASES.iter().any(|p| lc.contains(p)) {
        return FriendlyError::new(
            "Update couldn't be verified",
            "Companion downloaded an update but couldn't confirm it's legitimate.",
            "Your installer may be older than the current signing key. Re-download Companion from the link your project lead originally sent you to refresh it.",
            raw,
        );
    }
    if UPDATE_NETWORK_PHRASES.iter().any(|p| lc.contains(p)) {
        return FriendlyError::new(
            "Couldn't reach update server",
            "Companion can't reach the update server right now.",
            "Check your internet connection and try again. Companion will keep working in the meantime.",
            raw,
        );
    }
    if UPDATE_GENERIC_PHRASES.iter().any(|p| lc.contains(p)) {
        return FriendlyError::new(
            "Update check unavailable",
            "Companion couldn't complete the update check.",
            "This is usually temporary — Companion will keep working and try again next launch.",
            raw,
        );
    }
    if STREAM_CLIENT_PHRASES.iter().any(|p| lc.contains(p)) {
        return FriendlyError::new(
            "Project setup needs a refresh",
            "The workspace on the server is missing its stream binding.",
            "Click Force re-download in Advanced to rebuild the workspace and pull files fresh. (If this keeps happening, ask your project lead for a new invite.)",
            raw,
        );
    }
    if AUTH_PHRASES.iter().any(|p| lc.contains(p)) {
        return FriendlyError::new(
            "Your access expired",
            "Your access to this project has expired or was revoked.",
            "Ask your project lead for a fresh invite code.",
            raw,
        );
    }
    if TAILSCALE_KEY_USED.iter().any(|p| lc.contains(p)) {
        return FriendlyError::new(
            "Invite code already used",
            "This invite's connection key has already been used or has expired.",
            "Ask your project lead for a fresh invite code.",
            raw,
        );
    }
    if TAILSCALE_DOWN.iter().any(|p| lc.contains(p)) {
        return FriendlyError::new(
            "Connection service stopped",
            "Companion's connection service isn't running.",
            "Restart Companion. If the problem persists, restart your computer.",
            raw,
        );
    }
    if CONNECT_PHRASES.iter().any(|p| lc.contains(p)) {
        return FriendlyError::new(
            "Can't reach the project server",
            "Companion can't reach the server right now.",
            "Check that your internet is working, then try again. If this keeps happening, contact your project lead.",
            raw,
        );
    }
    if lc.contains("workspace") && (lc.contains("doesn't exist") || lc.contains("not owned")) {
        return FriendlyError::new(
            "Project setup is out of date",
            "This project's setup on the server doesn't match the invite.",
            "Ask your project lead for a fresh invite code.",
            raw,
        );
    }

    FriendlyError::new(
        "Something went wrong",
        "An unexpected problem stopped Companion from finishing.",
        "Try again. If it keeps happening, share the details below with your project lead.",
        raw,
    )
}

impl CompanionError {
    pub fn to_friendly(&self) -> FriendlyError {
        match self {
            CompanionError::InviteInvalid(msg) => FriendlyError::new(
                "Invite code can't be read",
                "The invite code looks corrupted or incomplete.",
                "Make sure you copied the entire code. If the problem persists, ask your project lead to resend it.",
                msg,
            ),
            CompanionError::InviteExpired => FriendlyError::new(
                "Invite has expired",
                "This invite is past its expiration date.",
                "Ask your project lead for a fresh invite code.",
                "expires_at is in the past",
            ),
            // v0.6.6 — p4-specific friendly mapping (must run BEFORE
            // `translate()` because translate's UPDATE_NETWORK_PHRASES
            // pool contains "timed out" which over-matches and
            // mislabels p4 timeouts as "Couldn't reach update server".
            // Operator saw exactly this during the Session 6 lock-storm:
            // every p4 command stalled at the 30s `CALL_TIMEOUT` and
            // the UI reported it as an update-server problem.
            CompanionError::Perforce(msg) => p4_friendly(msg),
            _ => translate(&self.to_string()),
        }
    }
}

/// p4-specific friendly mapping. Falls through to `translate()` if no
/// p4-shaped pattern matches.
fn p4_friendly(raw: &str) -> FriendlyError {
    let lc = raw.to_lowercase();

    // "p4 -c timed out after 30.0s" — produced by `run_p4`'s
    // `timeout(call_timeout, wait_with_output)` when a Perforce
    // operation overruns its budget. The fix is NOT "check your
    // internet" (the canonical UPDATE_NETWORK_PHRASES advice) —
    // it's "the Perforce server is overloaded or contested." Most
    // common cause: Unreal Editor and Companion both hammering the
    // same workspace at the same time. v0.6.3+ should make this
    // rare but defense in depth is cheap.
    if lc.contains("timed out after") {
        return FriendlyError::new(
            "Perforce server is slow",
            "A Perforce command took longer than expected and was canceled.",
            "Try again in a moment. If you have other apps using Perforce (Unreal Editor's Source Control panel, p4v, terminal) running a big operation at the same time, wait for them to finish. If this keeps happening on a quiet system, contact your project lead — the server may be under load.",
            raw,
        );
    }

    // v0.6.6 audit — tightened the lock-contention match. The bare
    // `"locked by"` substring was too broad (would catch any p4
    // message that mentions the word combination); the documented p4
    // stderr phrases for this case are `"can't be locked"` and the
    // verbatim quote `"already locked by"`.
    if lc.contains("can't be locked") || lc.contains("already locked by") {
        return FriendlyError::new(
            "File is locked by someone else",
            "Another teammate has this file checked out exclusively.",
            "Ask them to submit or revert their changes first, then retry.",
            raw,
        );
    }

    // Fall through to the generic translator for anything we don't
    // have a p4-specific pattern for (e.g. AUTH_PHRASES, CONNECT_PHRASES).
    translate(raw)
}

// Tauri commands return Result<T, String> by default; we serialize the
// friendly error as JSON in the string for the frontend to parse, or just
// pass the title string in simple cases. For richer flows we use a
// dedicated Result<T, FriendlyError> via serde.
impl Serialize for CompanionError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_friendly().serialize(serializer)
    }
}
