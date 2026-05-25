# Spaceshop Companion — Backlog

Queued work for future Companion sessions. Cleared as items ship; new items
land here whenever Arsen flags something or a session uncovers it. Most-recent
top.

## Shipped in v0.6

- **Clean uninstall feature** ✓ — "Uninstall Spaceshop Companion" button
  in Project → Advanced. Removes our line(s) from `%USERPROFILE%\p4tickets.txt`
  (preserves other apps' tickets), nukes `%APPDATA%\Spaceshop\` and
  `%LOCALAPPDATA%\Spaceshop\` recursively, optionally uninstalls Tailscale
  via `msiexec /x` (smart-default checkbox: ON only if we have a flag
  file proving we installed it, or if registry InstallDate matches
  Companion's same-day), and fires `msiexec /x {companion}` /passive
  before exiting. Falls back to `ms-settings:appsfeatures` if the
  ProductCode isn't in the Uninstall registry. Type-YES gated.
  See `src-tauri/src/commands/uninstall.rs` + `src/components/UninstallConfirm.tsx`.
- **Repair workspace button** ✓ — `p4 clean //...` (removes orphan files)
  + `p4 sync -f //...` (re-pulls fresh). Sits between Force re-download
  and Stop showing in Advanced. RepairConfirm modal explains the orphan-
  deletion semantics so users understand what gets removed (only files
  never added to p4 — opened-for-edit and pending changes survive).
  See `src-tauri/src/commands/perforce.rs::repair_workspace` +
  `src/components/RepairConfirm.tsx`.
- **Tray polling — color icon + per-project Sync Now + live tooltip** ✓
  — 30-second polling loop in `src-tauri/src/commands/tray_poll.rs`
  computes Tailscale-status + per-project `change_counts` health, tints
  the existing tray icon green/yellow/red via runtime luminance-weighted
  RGBA blend, rebuilds the menu dynamically with `sync-<project_id>`
  items for every onboarded project, and updates the tooltip with
  project/change-count summary. Tray Sync runs `sync_workspace`
  in-process and emits `pull-progress` events with `source: "tray"`
  so the main-window listener filters them out. Pulls/forces/repairs
  initiated from the main window now mark `TrayPollState.pulling` via
  a `PullingGuard` RAII so the tray poll doesn't collide with active
  syncs. Uses `tauri-plugin-notification` for completion toasts.
- **Background poll for All Projects status hints** ✓ — Welcome.tsx
  setInterval at 3 min re-runs `change_counts` per project, paused via
  `document.visibilityState` API. Refs track mounted state and in-flight
  fetches to prevent setState-after-unmount and duplicate spawns. Tiny
  `.checking-dot` opacity-pulse animation per tile while a fetch is in
  flight. Skips projects flagged `folder_missing`.
- **WiX util:CloseApplication fragment** ✓ — `src-tauri/wix/fragment.wxs`
  declares util:CloseApplication at Fragment level (NOT inside the
  Component — the v0.5.x attempt had a schema error) plus a placeholder
  anchor Component (`CompanionCloseAppAnchor` with HKCU RegistryValue
  KeyPath) referenced via `componentRefs` to force the fragment into
  the linked MSI. WiX's atomic fragment-linking pulls both. Now MSI
  upgrades close a running Companion.exe cleanly before replacing
  files — no more "files in use" prompts during in-app updater.
- **Banner primitive** ✓ — new `src/components/Banner.tsx` extracted
  from the inline-style banners in `UpdateBanner.tsx` and
  `StartupErrorBanner` (inside Shell.tsx). Severity-driven (info/warn/
  danger) with label/title/body/details/children slots. Refactor only;
  no behavior change. v0.7 polish could fold the uninstall-result and
  tray-sync-result surfaces into this primitive too.
- **App version display in header** ✓ — `Shell.tsx` fetches
  `getVersion()` on mount, renders `C O M P A N I O N · v 0 . 6 . 0`
  in the existing small-caps subtitle so the user can tell at a glance
  which build they're on. Survives auto-update because `getVersion()`
  reads the running binary's actual version.
- **Live pull-progress on Pull/Force/Repair buttons** ✓ — Project.tsx
  listens for `pull-progress` events filtered to its project_id and
  scoped to `source: "main"` (ignores `source: "tray"` events from
  tray-initiated syncs). Shows `{count} files · {truncated current
  filename}` below the badges while pulling. Mode label flips between
  "Pulling… / Re-downloading… / Repairing…" via `pullMode` state.
- **Update-error tech details disclosure** ✓ — when Check for updates
  fails, the friendly title shows in the status line and the raw
  `details` from Tauri's updater appears in a collapsed `<details>`
  panel so we can diagnose network-vs-signature failures without
  polluting the happy-path UI.
- **Fatal `startup-error` banner** ✓ — if `ensure_binaries` fails
  during app setup, lib.rs emits a structured event the frontend
  renders as a non-dismissable danger banner above the header with a
  Reinstall CTA. Replaces the previous silent `warn!` that left p4
  commands later spawn-failing with cryptic errors.
- **`invite::project_name` length + control-char validation** ✓ —
  rejects empty / >100 char / contains NUL/CR/LF/control. Prevents
  pathological menu labels and notification bodies if a malicious
  invite ever reaches us.
- **`-vnet.maxwait=300` on all p4 sync invocations** ✓ — protocol-level
  stall detection. Kills the sync only if the SOCKET is idle for
  5 minutes (p4 client error), not on slow legitimate downloads of
  big files. (Shipped in v0.5.9.)

## v0.6 build-infra notes

- **rustc 1.94 pin via `src-tauri/rust-toolchain.toml`** — eliminates
  random STATUS_ACCESS_VIOLATION crashes we saw on rustc 1.95.
- **`[profile.release.package.tauri] opt-level=1 + codegen-units=1`** —
  workaround for a deterministic rustc crash on the `tauri` crate at
  default opt-level=3 in v0.6.0. Combined with the same override
  on `spaceshop-companion`, all release builds compile cleanly.
- **Windows Sandbox smoke harness in `smoke/`** — `.wsb` config +
  `run.ps1` driver. Enable on the build host via
  `Enable-WindowsOptionalFeature -FeatureName "Containers-DisposableClientVM"
  -All -Online` + reboot. Then every release: copy MSI to `smoke/`,
  double-click `smoke.wsb`, grep `smoke/transcript.txt` for
  `SMOKE_TEST: PASS`. Catches install-time bugs before they reach a
  contractor. NOT YET ENABLED on the current build host — planned for
  the next reboot window.

## Up next (v0.7)

- **Real I/O byte-counter stall detection + Cancel button** — replace
  the current "no-timeout streaming sync" with actual bytes-flowing
  detection via Win32 `PROCESS_IO_COUNTERS`. Add a Cancel button so
  the user can abort a stuck sync gracefully. Win32 API path via the
  `windows` crate we already depend on.
- **`p4 -ztag` parser refactor** — switch streaming p4 output parsing
  from raw stdout regex to structured marshalled output. Real severity
  field (info/warn/failed/fatal) means we can distinguish "File(s)
  up-to-date" from "Client unknown" without our current substring
  hacks. Touches the whole streaming pipeline.
- **Welcome poll + tray poll dedup** — both fetch `change_counts` per
  project independently (Welcome every 3min visible, tray every 30s
  always). Wave-align occasionally → 2× p4 spawns. Either a shared
  cache with TTL in `TrayPollState`, or have Welcome consume tray-poll
  events instead of running its own loop.
- **Tactical filesystem-watcher byte-progress** — for big single-file
  Unreal asset downloads, watch the `.p4~` staging file size every 2s
  and derive bytes/sec to emit into `pull-progress`. ~50 LoC, purely
  client-side, bypasses the p4 CLI's lack of byte-level progress
  output. Alternative to the more invasive p4api/p4python linking.
- **File History view** — the ︙ menu in Changes view has "Show history…"
  grayed out. Implement: list of revisions per file, "Get this version"
  to roll back. Power-user op; keep the UI tight.
- **Logo in the installer** — the WiX MSI installer currently uses
  default Microsoft Installer dialogs with no Spaceshop branding.
  Drop a UI banner/dialog graphic into the WiX bundle so the install
  experience looks designed.
- **Larger Refresh button polish + UpdateBanner color hierarchy** —
  audit Finding #1 from the Wave 1 audit. The banner refactor lost
  the brighter contrast on `v{new_version}` that the old code had.
- **Defensive nonce-gate on `clean_uninstall_cmd`** — security
  agent Finding #1. Not currently exploitable (no XSS surface), but
  cheap defense-in-depth: generate a per-modal-open random nonce in
  Rust, require it as a second arg to the Tauri command. ~20 LoC.
- **`.p4ignore` phantom-edit fix** — `.p4ignore` shows as "modified" in
  the Changes view every sync because of CRLF↔LF mismatch on Windows
  (workspace uses `LineEnd: local`). Server-side typemap entry would fix
  it: `p4 typemap edit` and add `text+l //....p4ignore`. One-time
  operator action in Workshop, not a Companion code change.
- **`relocate_project` path validation** — `p4_relocate_project` takes a
  raw string and passes it to `PathBuf` with no validation. Low risk
  because Companion has no remote IPC, but a defensive check (must be
  absolute, must be under a user-writable root) would harden the surface
  if a malicious deep-link ever crafted a relocation URL.
- **Mark main-window pulls in `TrayPollState.pulling`** — Wave 2 audit
  H2 was partially addressed by `PullingGuard`, but verify under load
  that the tray poll properly skips a project that's actively syncing
  from the main window.

## Probably v0.7+

- **In-app file tree browser** — see project structure with colored
  status dots, right-click → Restore / Show history / Submit / Reveal.
  Currently we cover ~95% of "act on a file" use cases via the Changes
  view + Open Project Folder. Build this only if a real contractor asks.
- **Auto-update channel migration** — when GitHub Releases is settled
  and the NAS update endpoint is set up, ship a switchover release
  that points future updates at the NAS. Keep GitHub as a permanent
  fallback (see `docs/RELEASING.md` § Migration).
- **Code signing certificate** — get an Authenticode cert (~$200/yr)
  so SmartScreen stops yelling at contractors. Removes the "Run anyway"
  click from first-install UX.
- **DPAPI-wrapped state** — projects.json + the p4 ticket are plaintext
  now. Wrap with DPAPI so an attacker with local-disk access can't
  trivially exfiltrate the ticket. Modest defense-in-depth.
- **macOS support** — Tauri builds for macOS. When Spaceshop hires
  someone on a Mac. Mostly a packaging exercise; the Rust + React stays.

## Maybe

- **Multi-channel updates** (beta vs stable) — only if we get to the
  point of running pre-release candidates.
- **Companion writes Workshop a heartbeat** — so Workshop's PERFORCE
  admin tab can show "Sarah's Companion last checked in 3 min ago" for
  the operator. Requires a small Workshop HTTP endpoint.

## Won't do (cut, documented for re-litigation)

- **Built-in P4V-style depot browser** — out of scope; Unreal + Explorer
  cover navigation; in-app tree only if asked.
- **In-app diff viewer** — too much surface area; users open files in
  their native tool.
- **Submit-from-tray quick action** — submits without context are
  dangerous; force the contractor through the Changes view.
