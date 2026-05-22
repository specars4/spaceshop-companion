# Spaceshop Companion — Backlog

Queued work for future Companion sessions. Cleared as items ship; new items
land here whenever Arsen flags something or a session uncovers it. Most-recent
top.

## Up next (v0.6)

- **Background poll for status hints** — Companion currently knows what
  state a project was in *when last opened*. To make the All Projects
  page's "X changes on server / N local not sent" hints live, run
  `p4 changes -m1` + `p4 reconcile -n` periodically per project (every
  ~3 min when window is open, paused when minimized). Token cost is tiny.
- **"Open in Unreal" launcher** — discover the `.uproject` in the
  workspace and shell-launch it. Right now the button is in the daily
  view but stubbed.
- **Tray icon color polling** — every 30 s, set the tray icon's color to
  green / yellow / red based on Tailscale + Perforce reachability. Status
  pill currently only updates when the project page is open.
- **"Sync Now" tray menu item** — per the Session 1 amendments doc, tray
  should let you trigger a Pull on any project without opening the main
  window. Useful for non-Unreal review work.
- **File History view** — the ︙ menu in Changes view has "Show history…"
  grayed out. Implement: list of revisions per file, "Get this version"
  to roll back. Power-user op; keep the UI tight.

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
