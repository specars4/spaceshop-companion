# Spaceshop Companion — Session log

Chronological record of build sessions on the Companion app. Companion lives
in its own repo (`C:\LOCAL_PROJECTS\spaceshop-companion\`) and has its own
session/backlog cadence — separate from the larger SPACESHOP TOOLS project
because it ships independently to contractors as a `.msi` and has its own
release cycle.

Cross-link to the SPACESHOP TOOLS world only happens at the **invite-format
contract** (`docs/INVITE_FORMAT.md` in both repos; canonical source is
SPACESHOP TOOLS) and the **bundled `p4.exe`** (sourced from `tools/perforce/bin/`
in SPACESHOP TOOLS).

Most-recent first.

---

## Session 2 — 2026-05-20 / 2026-05-21 — v0.5 build + UX iteration + v0.5.2 follow-on

### Latest pass (2026-05-21, continuation) — v0.5.2 "Open in Unreal"

Big gold **Open in Unreal** hero CTA in the top-right of the project page,
next to the title. Click → Companion writes
`<project>/Saved/Config/WindowsEditor/SourceControlSettings.ini` (with the
project's server/user/workspace) **then** shell-launches the .uproject.
Unreal opens with Perforce already configured — no Editor Preferences
detour needed. The ticket is already in `%USERPROFILE%\p4tickets.txt`, so
Unreal authenticates with zero contractor input.

Window also shrunk from 880×640 to 860×600 (min 720×520) so the daily
project surface fits without scrolling on smaller displays.

Mechanism researched against Epic's UE 5.7 docs + community forum threads:
INI section names + key names confirmed (Port / UserName / Workspace).
The .uproject finder searches the workspace root + one level deep,
skipping engine cache dirs (Saved / Intermediate / DerivedDataCache /
Binaries / Build / .git / .p4).

### Earlier this session

**Outcome:** Working .msi installer end-to-end; mocked-and-agreed v2 daily-use
UI; ready to port to React and bake the updater in for the first contractor
release.

**Built in pass 1 (Session 2, 2026-05-20):**
- Full Tauri 2.x scaffold at this repo root
- Rust commands: invite parsing (mirrors `tools/perforce/invite.py` v=1),
  Tailscale lifecycle (bundled MSI + msiexec /qn install path), Perforce
  bootstrap (info / create_workspace / initial_sync / write_ticket_file),
  full onboarding chain with step events
- React frontend: Welcome / Confirm / Connecting / Project screens with
  Spaceshop brand styling
- System tray + hide-on-close + multi-project state persistence
- Produced `Spaceshop Companion_0.5.0_x64_en-US.msi` (37 MB)
- Smoke-tested end-to-end with `.scratch/perforce_session/test_invite_for_session_2.txt`
  from SPACESHOP TOOLS — tailscale up → p4 info → ticket write → workspace
  create → 3-file sync. One bug found and fixed: `p4tickets.txt` was
  read-only on existing Perforce users; Companion now clears the read-only
  bit before writing.

**Iteration pass 2 (2026-05-21):**
- Static HTML mockups in `mockups/` for iterating UI without
  install/rebuild round-trips. Five screens agreed:
  - **Daily v2** — status row + gold "N local not sent" / muted "X on
    server" badges + button bar + credentials and Advanced collapsibles +
    "‹ All projects" header nav
  - **Changes view** (renamed from Submit) — scrollable, master + per-row
    checkboxes, "Restore from server" replaces "Discard", ︙ per-row menu
    (Reveal / Restore / Show history grayed for v0.6 / Copy path)
  - **Conflict modal** — plain-language explainer, 3 options per file
    (Use server / Keep mine / Skip), pre-selected sensible default
  - **Force re-download confirm** — destructive modal with type-project-name
    gate, lists what will be lost
  - **All Projects v2** — no per-tile Pull, status hints per tile,
    "Status from last connection" for offline projects

**Now (Session 2 continuation, 2026-05-21):**
- Port the mockup designs into actual React + Rust
- Add the new Rust commands (list_changes / submit_changes / restore_file /
  force_resync / change_counts / conflict resolution)
- Bake in **Tauri updater plugin** + Ed25519 signing keypair so v0.5.1 (the
  first build sent to a real contractor) already self-updates from GitHub
  Releases
- Ship v0.5.1 .msi by end-of-session

See [`BACKLOG.md`](BACKLOG.md) for what's queued after v0.5.1.

---

## Session 1 — N/A

Companion didn't exist. Session 51 of SPACESHOP TOOLS built the Perforce
foundation (NAS p4d, Tailscale tailnet, invite generator in Workshop) that
this app talks to.
