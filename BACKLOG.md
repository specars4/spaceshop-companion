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
  See `src-tauri/src/commands/uninstall.rs` and
  `src/components/UninstallConfirm.tsx`.

## Up next (v0.6)

- **Background poll for status hints** — Companion currently knows what
  state a project was in *when last opened*. To make the All Projects
  page's "X changes on server / N local not sent" hints live, run
  `p4 changes -m1` + `p4 reconcile -n` periodically per project (every
  ~3 min when window is open, paused when minimized). Token cost is tiny.
- **Tray icon color polling** — every 30 s, set the tray icon's color to
  green / yellow / red based on Tailscale + Perforce reachability. Status
  pill currently only updates when the project page is open.
- **"Sync Now" tray menu item** — per the Session 1 amendments doc, tray
  should let you trigger a Pull on any project without opening the main
  window. Useful for non-Unreal review work.
- **File History view** — the ︙ menu in Changes view has "Show history…"
  grayed out. Implement: list of revisions per file, "Get this version"
  to roll back. Power-user op; keep the UI tight.
- **`.p4ignore` phantom-edit fix** — `.p4ignore` shows as "modified" in
  the Changes view every sync because of CRLF↔LF mismatch on Windows
  (workspace uses `LineEnd: local`). Server-side typemap entry would fix
  it: `p4 typemap edit` and add `text+l //....p4ignore`. One-time
  operator action in Workshop, not a Companion code change. Confusing
  noise in the contractor UX until it lands.
- **`relocate_project` path validation** — `p4_relocate_project` takes a
  raw string and passes it to `PathBuf` with no validation. Low risk
  because Companion has no remote IPC, but a defensive check (must be
  absolute, must be under a user-writable root) would harden the surface
  if a malicious deep-link ever crafted a relocation URL.
- **Better error classification for failed update checks** —
  `errors.rs::UPDATE_ENDPOINT_PHRASES` lumps signature-verification
  failures together with network failures, surfacing both as
  "Companion couldn't reach the update server." Misleading when the
  truth is "fetched it fine but the signature didn't verify against my
  embedded pubkey" (e.g., after a key rotation). Split into two
  distinct friendly messages: one for network, one for signature.
- **Byte-level progress for single large files** — current
  `pull-progress` events fire once per file *after* it finishes
  downloading. On a 50 GB Unreal `.uasset` the UI sits silent for 30+
  minutes and looks frozen. We investigated `p4 -I sync -q` in v0.6
  (see the long NOTE in `perforce.rs`) and confirmed it does NOT
  solve this — it reports file-count percent only, suppresses our
  per-file lines, and emits a terminal-style backspaced progress bar
  rather than parseable records. Real fix requires either linking
  p4api (C++) for `ClientProgress::Update()` callbacks, shelling out
  to p4python, or watching the on-disk file size of the in-progress
  download via an OS file-system watcher and inferring bytes/sec from
  delta. v0.7+ work.
- **`SYNC_TIMEOUT` may bite real projects** — currently hardcoded to
  1 hour in `perforce.rs`. A truly large initial sync (hundreds of GB
  on a typical home connection) will time out and the contractor is
  stuck on the error screen with no recovery path. Bump the timeout
  to 4–8 hours AND add a "resume sync" UI for when it does time out.

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
