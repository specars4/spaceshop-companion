# AGENTS.md — Spaceshop Companion

**This file is the load-bearing first read for any coding agent working
on Spaceshop Companion. Read it before doing anything else. The
constraints here override anything you find elsewhere unless explicitly
documented as superseding.**

Companion's parent-repo equivalent lives at
`C:\LOCAL_PROJECTS\Spaceshop_Perforce\SPACESHOP TOOLS\AGENTS.md` —
**read that one second** for cross-cutting Spaceshop conventions (brand
identity, invite v=1 schema authority, no-cloud rule, audit/fix-pipeline
patterns). This file covers only the **Companion-specific** rules that
don't apply to the broader Workshop+bridge codebase.

---

## What this is

Spaceshop Companion is the contractor-onboarding Tauri 2.x desktop app
that takes a contractor from "I have an invite code" to "Unreal's
Source Control panel shows green" in one button-click + one UAC prompt.
Single Windows-only `.msi` deliverable, signed and auto-updating via
GitHub Releases.

**Stack:** Tauri 2.11 (Rust 1.95 backend) + React 19 + TypeScript 5.8
(frontend) + Vite 7 (bundler). Single self-updating .msi installer.

**Scope:** bootstrap (Tailscale + Perforce + workspace + initial sync)
PLUS daily-use (Pull Latest, Changes view, Submit) PLUS Force
re-download. NOT in scope: anything Unreal-side (we hand off via a
COPY-THESE-INTO-UNREAL panel + write `SourceControlSettings.ini` and
let UE pick them up); admin operations (depot creation, invite
generation, user management — Workshop owns those); editorial features
(Companion is never the operator's main app).

## Hard rules (do not violate)

### Invite contract

- **The invite schema (v=1) is LOCKED.** `docs/INVITE_FORMAT.md` in this
  repo MUST stay byte-for-byte identical to the canonical mirror at
  `docs/INVITE_FORMAT.md` in the SPACESHOP TOOLS repo. The Workshop
  side generates; Companion side consumes. **Schema change ⇒ bump to
  v=2 with both versions coexisting, NOT a v=1 mutation.** Workshop's
  `InviteBuilder` and Companion's `parse_invite` are the two sides; a
  drift between them produces silent contractor-onboarding failures.
- **`tailscale.auth_key = ""` is a valid SELF-ONBOARD signal.**
  v0.6.1's `tailscale::up` short-circuits when the device is already
  on the tailnet (state Running/Started/Connected). Do not "fix" this
  by requiring a non-empty key.
- **`tag:contractor` is required** for contractor-onboarding invites
  per the Tailscale ACL. Workshop's invite generator enforces this on
  the producer side; Companion does NOT re-verify (the ACL does it
  server-side when the contractor's device joins the tailnet).

### Ticket + auth handling

- **`%USERPROFILE%\p4tickets.txt` is Unreal's expected ticket location.**
  Companion writes `<server>=<user>:<ticket>` lines into this file
  preserving any existing entries for other servers. Do NOT write to
  `P4TICKETS` env-var-overridden paths — Unreal Editor doesn't honor
  per-process env in the same way.
- **Clear the read-only bit before writing p4tickets.txt.** Existing
  Perforce users (re-onboarding) have this file marked read-only by
  prior p4 operations; without the clear, the write fails silently.
  Hit and fixed in Session 2.
- **`TICKET_FILE_LOCK` serializes p4tickets.txt mutations** across
  concurrent re-onboarding attempts. Don't bypass the lock; concurrent
  writes corrupt the file format.
- **`AUTH_PHRASES` in `src-tauri/src/commands/errors.rs` MUST match
  `tools/perforce/p4_client.py::_AUTH_ERROR_PHRASES` in SPACESHOP
  TOOLS verbatim** (lowercase, exact strings). Drift was a real bug —
  Companion was over-matching on the catch-all `"perforce password"`
  while Workshop classified more specifically, producing inconsistent
  "ask for a new invite" vs "auto-relog in" UX for the same p4d
  message. Whenever you touch one side, grep the other.

### Tailscale on Windows

- **Tailscale ships as MSI only on Windows.** Bundle the
  installer in `src-tauri/binaries/tailscale.msi`; install via
  `msiexec /i tailscale.msi /qn TS_UNATTENDEDMODE=always` invoked
  through PowerShell `Start-Process -Verb RunAs -Wait` so Windows
  fires a single UAC prompt the contractor has been pre-warned about.
  Do NOT try to bundle `tailscaled.exe` raw — Tailscale doesn't
  publish redistributable binaries.
- **`tailscale up --auth-key=""` errors out** unless the v0.6.1
  short-circuit path is taken (probe `status` first, return existing
  state if already joined). Workshop's SELF-ONBOARD invites rely on
  this — keep the short-circuit intact.
- **MSI 1619 workaround**: some Windows installations refuse to run
  the bundled MSI from a path containing spaces or non-ASCII
  characters. The fix is to copy the MSI to an 8.3 short-name temp
  directory before invoking msiexec; see `tailscale.rs::install_via_msi`.

### Perforce client topology

- **`Options: noallwrite noclobber nocompress unlocked nomodtime
  normdir`** is the canonical workspace options string. `noallwrite`
  is critical for Unreal (the editor silently writes to half the
  project; without `noallwrite` the "what's changed" detection
  breaks). Don't relax this without operator review.
- **`SubmitOptions: revertunchanged`** keeps Unreal's
  speculatively-opened files (open-then-no-change) from polluting
  changelists.
- **`LineEnd: local`** because Spaceshop is Windows-only. Do not
  switch to `share` or `unix`.
- **Stream-bound workspaces.** Companion creates clients with a
  `Stream: //<depot>/main` field; classic view-only workspaces are
  not used here. `ensure_workspace_stream_bound` auto-heals older
  workspaces created before stream binding shipped (Session 3).
- **`p4 sync //...` for daily Pull** (no `-f`); **`force_resync`
  (revert + flush + sync) for onboarding + re-onboarding** —
  "trust the server, nuke local state" semantics. Don't mix them.

### Errors

- **Every operator-visible error goes through `FriendlyError`
  (`errors.rs`).** Raw p4 stderr is for the log breadcrumb only.
- **No silent fallbacks.** If a thing can't happen, surface why.
- **Auth-key + ticket strings are scrubbed from error messages**
  before they reach the UI (defense-in-depth — `tailscale up`'s
  stderr can contain the key in some failure modes). Pattern lives
  in `tailscale.rs::up`.

### Build + release

- **Build is driven from SPACESHOP TOOLS**, not from inside this repo.
  The canonical command is:
  ```powershell
  cd C:\LOCAL_PROJECTS\Spaceshop_Perforce\SPACESHOP TOOLS
  powershell -ExecutionPolicy Bypass -File .\tools\perforce\build_companion.ps1 `
    -Notes "<release notes>" `
    -Publish
  ```
  The script handles version bumping cross-checks, signs the installer
  via the Ed25519 updater key, and pushes to GitHub Releases.
- **Don't run `npm run tauri build` from a Claude-Code-spawned shell.**
  Per the SPACESHOP TOOLS memory `project_dont_drive_long_builds_from_claude`,
  these long Cargo + bundling pipelines get reaped or stall in the
  harness. Hand to the operator.
- **Version bump locations** (must stay in lock-step on every release):
  `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `package.json`.
  All three.

## Conventions

### Rust (`src-tauri/`)

- Rust edition 2021. `async fn` on Tauri commands.
- Errors use `CompanionError` (thin enum over the categories: Tailscale
  / Perforce / Invite / Onboarding) and are translated to
  `FriendlyError` at the command boundary.
- Streaming progress events use the pattern: emit `pull-progress` /
  `onboarding-event` from inside the worker, frontend listens via
  `listen()`. See `perforce.rs::run_streaming_sync` for the canonical
  shape.
- `PullingGuard` is an RAII pattern — drop = "release the pull mutex."
  Don't refactor to manual release.

### TypeScript / React (`src/`)

- React 19, function components, hooks-only.
- Imports: `@tauri-apps/api/core` for invoke, `@tauri-apps/plugin-dialog`
  for file pickers. Don't shim them.
- Single source of truth for invoke wrappers: `src/lib/invoke.ts`. New
  Tauri commands add a typed wrapper there.
- Brand styles in `src/index.css` (palette + typography). Cream + gold
  + muted accents only; match the SPACESHOP TOOLS brand.

### State persistence

- `tauri-plugin-store` writes to
  `%APPDATA%\Spaceshop\Companion\projects.json`. **One entry per
  onboarded project.** Tickets are persisted in plaintext for v0.6 —
  same content is in `p4tickets.txt` anyway, so DPAPI wrapping would
  be theater. v0.7+ may wrap both.
- **New fields on `Project` MUST be `#[serde(default)]`**. v0.6.0 and
  earlier projects don't have them; without the default, loading
  errors out and the contractor loses their project list.

## Workflow

### Starting a session

1. Read this file.
2. Read `SESSIONS.md` (most-recent first) for what shipped + open
   threads.
3. Read `BACKLOG.md` for the queued work.
4. Read SPACESHOP TOOLS `AGENTS.md` for cross-cutting rules.
5. Confirm the task with the operator if there's any ambiguity.

### Audit / fix-pipeline conventions

Inherit the SPACESHOP TOOLS pattern (`tools/.../audit_2026_05/backups/`)
— see SPACESHOP TOOLS `AGENTS.md` "Audit / fix-pipeline conventions"
for the full pair-pattern + manifest schema + restore.py protocol.
Backups for Companion-side fixes live at
`C:\LOCAL_PROJECTS\Spaceshop_Perforce\SPACESHOP TOOLS\.scratch\audit_2026_05\backups\<batch_id>\`
**in the SPACESHOP TOOLS repo**, NOT inside the Companion repo. The
`original` field in MANIFEST.json uses a `../spaceshop-companion/...`
path prefix.

**Light vs heavy pair-pattern:** scale to operator availability per the
SPACESHOP TOOLS memory `feedback_agent_intensity_scales_with_presence` —
single-file ≤30 LOC fixes solo + parse-check + restore-dry-run, no
agents. Multi-file medium = solo + one consolidated sanity-check
agent at the end. Cross-repo schema changes = full pair pattern.

### Ending a session

1. Update `SESSIONS.md` with a new entry (most-recent first). Keep under
   ~250 words; link files; document what shipped + open threads.
2. If a new pattern, decision, or constraint emerged that future
   Companion-side sessions need: add it here.
3. If the version bumped, ensure all three locations
   (`tauri.conf.json`, `Cargo.toml`, `package.json`) match.

## Asking the operator

The operator (Arsen) is responsive and gives good direction. Default to
asking when:

- A task has multiple reasonable interpretations.
- An architectural decision needs to be made that's not in any doc.
- A hard rule from this file would need to be broken.
- You're about to publish a release.

Don't ask for trivia or for confirmation of obvious next steps. Don't
ask the same question twice in a session.

## Last word

Companion is the contractor-facing surface for everything Spaceshop's
Perforce setup does. Bugs here erode contractor trust; over-engineering
here adds maintenance load to a single-developer studio. When in doubt,
favor "ship the minimum that works" — the operator will tell you if
you need more.
