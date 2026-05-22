# Spaceshop Invite Code Format — v1

**Status:** Locked. Session 2's Companion app builds against this contract.

This document specifies the on-the-wire format Spaceshop Workshop emits when
generating contractor invites, and what Spaceshop Companion consumes on the
contractor's side. Both implementations MUST match this spec exactly. If the
spec changes, the `v` field bumps and old and new versions coexist.

## TL;DR

An invite is a base64-url-safe-encoded UTF-8 JSON object describing
everything a contractor's Companion app needs to:

1. Join the Spaceshop tailnet (single-use ephemeral key, `tag:contractor`)
2. Authenticate against the Perforce server (server address, username, ticket)
3. Build their workspace and pull the project (workspace template + local root)

The invite is **self-contained** — no Companion-server roundtrip required.
The contractor pastes the invite into Companion (or clicks a
`spaceshop-companion://invite/{code}` URL) and Companion has everything it
needs to set up the connection.

## Schema (v=1)

```json
{
  "v": 1,
  "project_name":  "Pharma Spot — Neymarc",
  "project_id":    "pharma-neymarc-2026-q2",
  "issued_at":     "2026-05-20T14:00:00Z",
  "expires_at":    "2026-08-20T14:00:00Z",
  "issued_by":     "arsen@spaceshop.studio",
  "perforce": {
    "server":      "100.82.0.8:1666",
    "user":        "sarah-contractor",
    "ticket":      "ABC123DEF456...",
    "workspace_template": {
      "name":      "sarah-pharma-neymarc",
      "view": [
        "//pharma-neymarc/main/... //sarah-pharma-neymarc/..."
      ],
      "options":   "noallwrite noclobber nocompress unlocked nomodtime normdir"
    },
    "local_root_default": "%USERPROFILE%\\SpaceshopProjects\\PharmaNeymarc"
  },
  "tailscale": {
    "auth_key":    "tskey-auth-xxx...",
    "tags":        ["tag:contractor"]
  }
}
```

### Field reference

| Field | Type | Required | Notes |
|---|---|---|---|
| `v` | int | yes | Schema version. **This document defines v=1.** Companion MUST refuse to parse a code with an unsupported `v`. |
| `project_name` | string | yes | Human-readable project name. Displayed to the contractor. |
| `project_id` | string (kebab) | yes | Stable identifier matching the depot name. ASCII, lowercase, `[a-z0-9-]`. |
| `issued_at` | ISO 8601 | yes | UTC timestamp the invite was generated. Mirrors `expires_at` timezone-handling. |
| `expires_at` | ISO 8601 | yes | UTC timestamp after which Companion MUST refuse to apply the invite. Workshop's invite generator defaults this to `issued_at + 90 days`. |
| `issued_by` | email | yes | The admin who issued. Surfaced in Companion's invite confirmation screen so the contractor knows who they're trusting. |
| `perforce.server` | `host:port` | yes | Address Companion connects to. Workshop's generator defaults to the NAS's Tailscale IP (works from anywhere on the tailnet); operator can override to LAN IP for in-studio testing. |
| `perforce.user` | string | yes | The Perforce username Workshop has provisioned for this contractor. |
| `perforce.ticket` | string | yes | A pre-minted P4 ticket for the contractor. Avoids the contractor needing the underlying password. Tickets are scoped to the user; revoking the user in Perforce invalidates the ticket. |
| `perforce.workspace_template.name` | string | yes | Workspace name template. Companion creates the workspace using this name (or appends a host suffix if a collision is detected). |
| `perforce.workspace_template.view` | string[] | yes | The workspace view lines. Companion writes these verbatim into the workspace spec. |
| `perforce.workspace_template.options` | string | yes | Workspace options line. Default: `noallwrite noclobber nocompress unlocked nomodtime normdir`. |
| `perforce.local_root_default` | string | yes | The local-disk path Companion offers as the default workspace root. Contractor may override during onboarding. Use `%USERPROFILE%` placeholders for Windows; Companion expands at apply time. |
| `tailscale.auth_key` | string | yes | A single-use, pre-approved, ephemeral Tailscale auth key, tagged `tag:contractor`. Generated at https://login.tailscale.com/admin/settings/keys per invite. |
| `tailscale.tags` | string[] | yes | Tags the contractor's device gets. **Must include `tag:contractor`** (ACL-enforced — see `tailscale_acl.json`). |

### Encoding

The JSON payload is:

1. Serialized to UTF-8 with no extra whitespace (`json.dumps(payload, separators=(",",":"))` in Python; `JSON.stringify(payload)` with no indent in JS).
2. Base64-url-safe-encoded **without padding** (`base64.urlsafe_b64encode(...).rstrip(b'=')` in Python; `btoa(...).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'')` in JS).
3. The resulting ASCII string IS the invite code.

### Distribution forms

The same code string can ship to a contractor any of three ways:

- **Plain text:** the contractor pastes the string into Companion's input.
- **URL:** `spaceshop-companion://invite/{code}` — Companion registers this
  custom URL scheme during install; clicking the link opens Companion with
  the code pre-loaded.
- **Downloadable file:** `*.spaceshop-invite.json` — contractor double-clicks
  to launch Companion with the embedded code.

Workshop's generator UI offers all three.

## Security properties

- **Treat invites as bearer tokens.** The code contains the contractor's
  Perforce ticket plus an active Tailscale auth key. Anyone holding the
  full code can connect as that contractor until either (a) `expires_at`
  passes, (b) the Perforce user is removed, or (c) the Tailscale key is
  revoked. Hand off via Signal, encrypted email, or password manager —
  never plaintext public channels.

- **No phone-home.** Companion does not contact a Spaceshop server to
  validate the invite. The expiration check is purely local against
  `expires_at`. (This is a v0.5 trade-off documented in `RUN_COMPANION_KICKOFF.md`.)

- **Revocation path.** Workshop's user-management UI (Track C.5) lists
  active users and can disable a user (`p4 user -d`), which invalidates
  the ticket regardless of `expires_at`. Tailscale auth keys are revoked
  separately at https://login.tailscale.com/admin/settings/keys.

- **Ephemeral keys auto-expire.** When the contractor disconnects from
  Tailscale (e.g., uninstalls Companion, shuts down their machine for >
  the ephemeral timeout), the auth key cannot be re-used to rejoin.

## Workshop side — what generates this

`tools/perforce/invite.py` (Track C.7 implementation):

- `build_invite_payload(...) -> dict` — assembles the canonical dict from
  loose params; sets `issued_at` to `now()` and `expires_at` based on a
  TTL (default 90 days).
- `encode_invite(payload: dict) -> str` — runs the serialization + base64
  per the encoding spec above.
- `decode_invite(code: str) -> dict` — inverse for testing.

The invite generator UI (Track C.7) calls `p4_client.issue_ticket_for(user)`
to mint the `perforce.ticket` field — Workshop's admin runs as a super-user
who can issue tickets on behalf of other users without their password.

## Companion side — what consumes this

Session 2 will implement `commands/invite.rs` (Rust):

- `parse_invite(code: String) -> Result<InviteData, Error>` — base64
  decode + JSON parse + schema validate against this `v=1` spec.
- Validation rules:
  - `v == 1` (refuse otherwise)
  - `expires_at` is in the future (refuse otherwise)
  - All required fields present
  - `tailscale.tags` contains `tag:contractor`
  - `perforce.server` matches `^[a-z0-9.-]+:[0-9]+$` (basic sanity)

If you change this spec, bump `v` to `2` and Workshop must continue to
EMIT v=1 codes for a transition period while Companion learns to read
both.
