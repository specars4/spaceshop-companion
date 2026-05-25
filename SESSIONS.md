# Spaceshop Companion — Session log

Chronological record of build sessions on the Companion app. Companion
lives in its own repo (this one) and has its own session/backlog cadence
— separate from the larger SPACESHOP TOOLS project because it ships
independently to contractors as a `.msi` and has its own release cycle.

Cross-link to the SPACESHOP TOOLS world only happens at the **invite-format
contract** (`docs/INVITE_FORMAT.md` in both repos; canonical source is
SPACESHOP TOOLS) and the **bundled `p4.exe`** (sourced from `tools/perforce/bin/`
in SPACESHOP TOOLS).

Most-recent first.

---

## Session 3 — 2026-05-22 — marathon v0.5.4 → v0.6.0 + agent-orchestrated build process

**Outcome:** Started the session on v0.5.3 (signed, published, working).
Shipped v0.5.4 → v0.5.5 → v0.5.6 → v0.5.7 → v0.5.8 → v0.5.9 → v0.6.0
in one session, mostly via multi-agent parallel-fix dispatch with
audit-pair review between waves. Contractor's full onboarding +
auto-update path verified end-to-end on real hardware (not just the
dev box).

### v0.5.4 — bug-fix backlog from Session 2
- Parser handles `submit change <N> to ...` lines in `p4 status`
  (previously only `reconcile to ...` matched, so files already in a
  pending changelist were silently skipped from the Changes view)
- `submit_changes` calls `ensure_workspace_stream_bound` first —
  auto-heals workspaces created before stream-binding support
- Pull Latest always clickable, shows "Re-check & pull" when remote
  has no changes
- `force_resync` does `revert -k //...` → `flush //...#0` → `sync //...`
  instead of bare `sync -f` — handles stuck "open for delete" files
- `initial_sync` delegates to `force_resync` so re-onboards heal
- Update-check spam suppressed when endpoint is a placeholder
- Footer removed, YES gates on destructive actions, "SERVER DETAILS"
  label instead of "COPY THESE INTO UNREAL"
- Generic-token redaction across docs (`<SPACESHOP_TOOLS_ROOT>` etc.)
- Dropped `tauri-plugin-shell` + `tauri-plugin-opener` (dead)
- USER_GUIDE.md + CONTRACTOR_ONBOARDING_TEMPLATE.md rewritten for
  the v0.5.2 Open-in-Unreal flow

### Repo rename — `spaceshop-companion` → `onboard`
- Public URL: https://specars4.github.io/onboard/
- Repo: https://github.com/specars4/onboard
- All updater endpoints updated; GitHub's repo-rename redirect
  covers old URLs in the wild

### v0.5.5–v0.5.8 — onboarding install-path fixes from contractor reports
- v0.5.5: split UPDATE_ENDPOINT_PHRASES into NETWORK / VERIFY /
  GENERIC so Check-for-updates can report the actual cause
- v0.5.6: Tailscale msiexec `WaitForExit` (was racing `Select-Object
  -ExpandProperty ExitCode`)
- v0.5.7: copy bundled tailscale.msi to %TEMP% before msiexec to
  dodge "exit 1619 ERROR_INSTALL_PACKAGE_OPEN_FAILED" on paths with
  spaces (`C:\Program Files\Spaceshop Companion\...`); also Win32
  GetShortPathName fallback for spacey usernames
- v0.5.8: streaming-sync wall-clock timeout REMOVED — was killing
  big-file syncs that legitimately took >1h; replaced with idle-only
  semantics (later refined to no timeout + `-vnet.maxwait=300`).
  Fixed Wix `upgradeCode` for clean MajorUpgrade. 8.3 short-name
  fallback. Larger Refresh button.
- v0.5.9: backend hardening pack (10 fixes including ensure_binaries
  startup-error event, explorer.exe path escape, workspace_root
  guards on all p4 entry points, tailscale JSON parse defensive,
  p4 stderr classification, find_uproject symlink safety,
  write_ticket_file Mutex, temp MSI per-process name,
  `-vnet.maxwait=300`, errors.rs UPDATE_NETWORK/VERIFY/GENERIC split).
  Frontend: version in header, live pull-progress, error-details
  disclosure, fatal startup-error banner. Build: rustc 1.94 pin,
  Tauri 2.11 pin, NSIS dropped. Windows Sandbox smoke harness.

### v0.6.0 — feature wave (Wave 1 + Wave 2 + audit fixes)
- WiX `util:CloseApplication` fragment re-enabled (top-level
  Fragment + anchor Component pattern, `componentRefs` reference
  to satisfy Tauri bundler's link-or-drop semantics)
- "Repair workspace" button (`p4 clean` + `sync -f`) with confirm
  modal explaining orphan-deletion
- Clean uninstall feature with smart Tailscale detection
  (install-time flag + YYYYMMDD InstallDate fallback), scrubs
  p4tickets line, deletes Spaceshop data dirs, optional Tailscale
  uninstall, self-uninstalls via `msiexec /x`
- Tray polling: runtime icon tinting (luminance-weighted RGBA
  blend, no extra crate), 30s loop, per-project Sync Now menu,
  live tooltip
- Welcome background poll every 3 min with visibility-pause
- Shared `<Banner>` primitive (UpdateBanner + StartupErrorBanner)
- `invite::project_name` length + control-char validation
- `[profile.release.package.tauri] opt-level=1, codegen-units=1`
  — workaround for deterministic rustc crash on tauri lib at
  default opt-level=3

### Process changes
- **Multi-agent parallel fix dispatch** — most v0.6 work done by
  4-agent waves (each owning a self-contained slice), audited by
  a 2-agent pair (integration + security/correctness) between
  waves. Worked well; iteration time was ~30 min per wave including
  audit + integration + build.
- **VM testing infra shipped** but not yet enabled (`smoke/` folder).
  Activate by enabling Windows Sandbox on the build host.
- **Hot-fix-machine pattern called out** — earlier in session we
  kept blindly retrying rustc crashes; switched to actually pinning
  the toolchain and applying targeted opt-level overrides.

### What the contractor sees in v0.6.0
- Banner update: `v0.5.7 → v0.6.0` on next launch (or via Advanced
  → Check for updates manually)
- Header now shows running version
- Repair workspace + Clean uninstall buttons in Advanced section
- Color-coded tray icon (green / yellow / red)
- Per-project Sync from tray menu
- Live `{count} files · current filename` during pull
- Auto-update technical details disclosure if Check fails

### Still queued for v0.7
See BACKLOG.md "Up next (v0.7)" — Cancel button + Win32 I/O byte
counters, `-ztag` parser refactor, Welcome+tray poll dedup,
filesystem-watcher byte progress, File History view, installer logo,
defensive nonce on clean_uninstall, etc.

---

## Session 2 continuation — 2026-05-21 evening — daily-ops + ship infrastructure

**Outcome:** v0.5.3 shipped to GitHub (public repo, signed, auto-update verified
end-to-end). v0.5.4 attempted with critical bug fixes but build pipeline
stalled — needs fresh session to finish.

### What landed

- **GitHub repo live + public**: https://github.com/specars4/onboard
- **v0.5.3 release published with signed MSI**: https://github.com/specars4/onboard/releases/tag/v0.5.3
- **Auto-update endpoint verified** — anonymous fetch returns valid signed
  manifest, MSI URL resolves
- **GitHub Pages branch pushed** (`gh-pages`) — landing page at
  `https://specars4.github.io/onboard/` with single big "Install
  Companion" button + SmartScreen guidance + JS that auto-fetches the
  latest release. Needs ~60s after push to go live; verify when next
  session starts.
- **Signing key** moved into the operator's local SPACESHOP TOOLS
  workspace under `tools/_secrets.py` as `COMPANION_UPDATER_PRIVATE_KEY`.
  Local `.keys/` deleted from the Companion repo.
- **One-command publish helper**: `<SPACESHOP_TOOLS_ROOT>/tools/perforce/build_companion.ps1`
  — reads the key, runs `tauri build`, optionally cuts the GitHub Release.
  Had a bug (empty-string PowerShell args + Python -c indent + em-dash
  chars in source) — all fixed. Should work in a fresh shell.

### Bug fixes between v0.5.3 → intended-v0.5.4 (in git, NOT yet in a published .msi)

| Bug | Where | Fix |
|---|---|---|
| `p4 status` parser only handled `reconcile to` lines, skipped `submit change N to add` lines | `list_changes` | Now matches both formats |
| Submit fails on pre-fix workspaces with "cannot submit from non-stream client" | `submit_changes` | Calls `ensure_workspace_stream_bound` before reconcile/submit; auto-heal preserves existing spec, adds Stream binding derived from view |
| Pull Latest button greyed when no remote changes — felt broken | `Project.tsx` | Always clickable, shows "Re-check & pull" when nothing new |
| Update-check spammed ERROR every launch + every click while endpoint was placeholder | `updater.rs` | Detect placeholder, skip silently with one info log |
| Footer "ARSEN ARZUMANYAN..." unwanted | `Shell.tsx` | Removed |
| Confirm gates required typing project name | `ForceResyncConfirm`, Project Remove inline | Now require typing `YES` |
| "COPY THESE INTO UNREAL" label too prescriptive | `Project.tsx` | Renamed to "SERVER DETAILS" with softer caption |

### Server-side: contractors group Timeout was 12h → bumped to 90 days

`set_contractor_group_timeout(client, days=90)` added to
`tools/perforce/admin.py`; ran live against NAS p4d. Smoke invite now has
90-day ticket. Existing tickets minted before the change keep their old
lifetime (Perforce reuses, doesn't refresh). For full reset run
`regenerate_smoke_invite.py` which does logout-then-login.

### What broke in the last hour (root cause: trying to do too much in one Claude session)

1. **rustc STATUS_ACCESS_VIOLATION (0xc0000005)** during incremental release
   builds — intermittent rustc 1.95 / MSVC bug. Cleared
   `target/release/incremental` once, helped temporarily.
2. **Tauri `signer sign` CLI hangs** when invoked outside a fully-interactive
   shell; prompts for confirmation that never comes. Use `npm run tauri
   build` with env vars set — that path produces .sig reliably.
3. **PowerShell strips empty-string args** to native exes, so `-p ""` becomes
   `-p` with no value. Either use `[Parameter]` flag syntax (`--no-password`)
   or omit the flag and rely on the env var.
4. **My background-task timeouts** were swallowing in-flight `npm run tauri
   build` runs. The next session should run the build in a real
   foreground PowerShell terminal (not via Claude's tool subprocess) so
   nothing reaps it.

### Current artifact state

- `src-tauri/target/release/bundle/msi/Spaceshop Companion_0.5.4_x64_en-US.msi`
  exists (39 MB) but is **unsigned** (.sig missing). Don't ship this.
- v0.5.3 .msi + .sig are both fine and on the GitHub Release. Safe to
  send contractors today if necessary.

### What the next session should do (in order)

1. **Verify GitHub Pages is live** — visit
   `https://specars4.github.io/onboard/`. If 404, wait 5
   minutes; if still 404, check Pages settings via `gh api /repos/specars4/onboard/pages`.
2. **Publish v0.5.4** — in a real PowerShell terminal (not through Claude
   tools), run:
   ```powershell
   cd <SPACESHOP_TOOLS_ROOT>
   powershell -ExecutionPolicy Bypass -File .\tools\perforce\build_companion.ps1 `
     -Notes "Parser handles pending-changelist files; submit auto-heals stream binding; pull latest always clickable." `
     -Publish
   ```
   If rustc crashes, `cd <COMPANION_REPO_ROOT>\src-tauri; cargo clean`
   (full clean, not just incremental), then re-run.
3. **Smoke-test end-to-end** — install v0.5.4 .msi on Arsen's machine,
   paste the smoke invite, walk through:
   - Onboarding succeeds
   - Make a file → Review & submit → succeeds (stream auto-heal)
   - Re-check & pull → green banner
   - Auto-update banner doesn't fire (we're on latest)
4. **Ship to Arsen's contractor** — send them:
   - URL: `https://specars4.github.io/onboard/`
   - The invite code generated via Workshop's PERFORCE → INVITES panel
     (or `tools/perforce/regenerate_smoke_invite.py` for smoke testing)

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
