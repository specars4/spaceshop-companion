# Spaceshop Companion v0.5.1 — Build Handoff

**Built:** Session 2 (2026-05-20 → 2026-05-21)
**Driving docs:** `RUN_COMPANION_KICKOFF.md` + `RUN_COMPANION_KICKOFF_AMENDMENTS.md`
  in the SPACESHOP TOOLS repo
**Invite contract:** `docs/INVITE_FORMAT.md` v=1 in SPACESHOP TOOLS
**Release procedure:** [`docs/RELEASING.md`](RELEASING.md) — how to cut new versions
**Session log:** [`SESSIONS.md`](../SESSIONS.md) at the repo root

## v0.5.1 — what's new since v0.5.0

After v0.5.0's bootstrap-only build, v0.5.1 adds the daily-use surface and
self-update infrastructure so the first build sent to a real contractor
can update itself without a re-install:

- **Project Home redesign:** status row (connection / last download /
  folder) + side-by-side badges ("N local changes not sent" gold, "X
  changes on server" muted) + button bar with Pull-latest / Open-folder /
  Open-in-Unreal-stub. Credentials and Advanced (Force re-download +
  Remove project) collapse out of the way.
- **Changes view:** scrollable file table with master + per-row
  checkboxes, M / + / − glyphs for modified / added / deleted, selection
  bar with Select-all / -none / -only-modified, per-row ︙ menu with
  Reveal-in-Explorer / Restore-from-server / Show-history (grayed for
  v0.6) / Copy-path, description field, "Submit N files" button. Reached
  from the gold badge on Project Home.
- **All Projects redesign:** per-tile status hint
  ("3 local not sent · 7 on server" / "Nothing new" / "Status from last
  connection"). Per-tile Pull removed — Pull happens inside a project
  where you can see context. Background poll refreshes counts every 3 min.
- **Force re-download confirm dialog:** destructive modal, type-the-project-name
  gate, lists which local changes will be lost.
- **Tauri updater + GitHub Releases:** on launch Companion checks
  `https://github.com/<org>/spaceshop-companion/releases/latest/download/latest.json`,
  shows a non-intrusive banner at the top if a newer version is available,
  Install button → silent download + install + auto-restart. Ed25519
  signed; private key lives in Arsen's password manager.

## What was built

A standalone Tauri 2.x desktop app at `C:\LOCAL_PROJECTS\spaceshop-companion\`
that takes a contractor from "I have an invite code" to "Unreal's Source
Control panel shows green" with no jargon and no follow-up messages from
Arsen.

**Stack:** Tauri 2.11 / Rust 1.95 (backend) + React 19 / TypeScript 5.8
(frontend) / Vite 7 (bundler). Single .msi (or NSIS .exe) installer.

**Per the AMENDMENTS doc, scope was simplified vs the original kickoff:**

| Original kickoff | Built? |
|---|---|
| Paste-or-click invite + URL handler | ✅ Yes |
| Pick local folder | ✅ Yes |
| Silent Tailscale install | ✅ Yes (bundled MSI, one UAC prompt) |
| `p4 sync` initial download | ✅ Yes |
| Paste-into-Unreal credentials panel | ✅ Yes (centerpiece of the UI) |
| Tray icon + Open Folder + Status | ✅ Yes |
| Submit / file-list / conflict UI | ❌ **Cut** (per AMENDMENTS — Unreal handles checkouts/submits) |
| Sync-Now main daily action | ❌ **Cut** (Unreal pulls; tray "Sync Now" deferred to v0.6) |

**Result:** Companion is bootstrap-only. After onboarding it gets out of
the way — daily Perforce ops happen in Unreal (contractors) or in
Workshop's PERFORCE tab (Arsen). The big "COPY THESE INTO UNREAL" panel
is the centerpiece, with Server / User / Workspace each one-click copyable.

## File layout

```
spaceshop-companion/
├── src/                                # Frontend (React + TS)
│   ├── App.tsx                         # State-machine router
│   ├── main.tsx                        # React entry
│   ├── index.css                       # Spaceshop brand styles
│   ├── lib/
│   │   ├── invoke.ts                   # Typed wrappers around Tauri commands
│   │   └── types.ts                    # Mirrors Rust types
│   ├── components/
│   │   ├── Shell.tsx                   # Header (gold-bullet wordmark) + footer
│   │   ├── CopyField.tsx               # Paste-into-Unreal line w/ copy button
│   │   ├── StepList.tsx                # Onboarding progress
│   │   └── ErrorBox.tsx                # FriendlyError surface
│   └── pages/
│       ├── Welcome.tsx                 # Paste invite + list onboarded projects
│       ├── Confirm.tsx                 # Review invite + pick folder
│       ├── Connecting.tsx              # Bootstrap progress + sync output
│       └── Project.tsx                 # Paste-into-Unreal panel + Open Folder
├── src-tauri/                          # Rust backend
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── capabilities/default.json       # Tauri 2.x permission allowlist
│   ├── binaries/
│   │   ├── p4.exe                      # 9.4 MB — bundled CLI (copy of tools/perforce/bin/p4.exe)
│   │   └── tailscale.msi               # 35 MB — Tailscale 1.98.2 installer
│   └── src/
│       ├── main.rs                     # Entry → lib::run()
│       ├── lib.rs                      # Builder, plugins, tray, deep-link wiring
│       └── commands/
│           ├── mod.rs
│           ├── errors.rs               # CompanionError + raw→friendly translation
│           ├── invite.rs               # Mirrors tools/perforce/invite.py (v=1)
│           ├── tailscale.rs            # status / up / install_via_msi
│           ├── perforce.rs             # info / create_workspace / initial_sync / write_ticket_file
│           ├── paths.rs                # Bin paths, p4tickets.txt location
│           ├── bundled.rs              # First-run binary copy
│           ├── projects.rs             # Persisted state via tauri-plugin-store
│           └── onboarding.rs           # The full bootstrap chain
└── docs/                               # This folder
```

## Key technical decisions

### 1. Tailscale as a bundled MSI, not raw binaries

Tailscale's only Windows distribution is the installer; they don't publish
redistributable `.exe` files. On first run Companion invokes
`msiexec /i tailscale.msi /qn TS_UNATTENDEDMODE=always` via PowerShell
`Start-Process -Verb RunAs -Wait` so Windows fires a single UAC prompt
the contractor has been pre-warned about. After install, Tailscale lives at
its standard path `C:\Program Files\Tailscale\tailscale.exe` and Companion
calls it from there.

This is cleaner than the kickoff's "bundle tailscaled.exe + run install-service"
approach because the MSI registers the Windows service correctly, handles
upgrades naturally, and gives the contractor a normal-looking install if
they ever poke around in Add/Remove Programs.

### 2. Workspace settings — Unreal-friendly

The invite specifies `Options: noallwrite noclobber nocompress unlocked
nomodtime normdir` and Companion uses it verbatim. On top of that,
Companion adds:

- `SubmitOptions: revertunchanged` (Unreal opens many files speculatively;
  this keeps changelists clean)
- `LineEnd: local` (Spaceshop is Windows-only; no cross-platform churn)

Server-side typemap (set up in Session 1's foundation work) ensures
`.uasset / .umap / .uproject / .uplugin / .psd / .tga / .tif` get
`binary+l` — exclusive lock. Two contractors can't both check out the
same Unreal asset because binaries can't merge.

### 3. Ticket written to `%USERPROFILE%\p4tickets.txt`

Unreal's Perforce provider reads the standard p4tickets file with no env
var fiddling. Companion writes the contractor's ticket there directly
(format: `<server>=<user>:<ticket>` per line, preserving any existing
entries for other servers). After Companion runs:

- Open Unreal
- Editor Preferences → Source Control → Provider: Perforce
- Paste the three values from Companion's panel
- Click Accept Settings
- Unreal turns green ✓

No password prompt, no per-machine config.

### 4. Hide window on X, quit only via tray

Closing the window with the X button hides it; Companion stays alive in
the system tray and reopens on tray-icon click. Quit only via tray menu →
"Quit". Matches Slack / Discord / typical Win11 background utility
behavior.

### 5. State persistence

`tauri-plugin-store` writes to `%APPDATA%\Spaceshop\Companion\projects.json`.
One entry per onboarded project. **Tickets are stored in plaintext** for
v0.5 — same content is in `p4tickets.txt` anyway, so DPAPI wrapping would
be theater. v0.6 will wrap both consistently.

## How it runs end-to-end

1. Contractor receives `spaceshop-companion://invite/{base64}` link from
   Arsen (or the raw code; pasting works too).
2. Contractor double-clicks the .msi installer Arsen sent. Installs to
   per-user location, registers the `spaceshop-companion://` URL handler,
   adds Start Menu entry + Spaceshop Companion to tray.
3. First launch (auto-opens after install, or contractor opens it):
   - Onboarding screen asks for the invite code (pre-filled if launched
     via the URL link).
   - Contractor reviews "Setting up <project name>", picks (or accepts)
     the project folder.
   - Clicks **Connect**. Companion runs the 7-step bootstrap chain
     (parse → service → connect → server → ticket → workspace → sync),
     streaming progress to the UI.
   - One UAC prompt appears (for the Tailscale MSI install). Pre-warned
     copy: "Windows will ask permission once. Click Yes."
4. After ~2-5 min the success screen shows the COPY-THESE-INTO-UNREAL
   panel with Server / User / Workspace + copy buttons + the path to the
   project folder.
5. Contractor opens Unreal, pastes the three values into Editor
   Preferences → Source Control, clicks Accept Settings. Unreal validates
   and turns green. Daily Perforce work happens entirely inside Unreal
   from this point on.

## What's NOT in v0.5

- **Code signing** — installer is unsigned. SmartScreen will warn; the
  contractor onboarding email template tells them to click "More info" →
  "Run anyway".
- **Auto-updates** — ship a new .msi to upgrade.
- **macOS** — Windows only.
- **Submit / file-list UI** — cut per AMENDMENTS; Unreal owns it.
- **"Sync Now" in the tray menu** — code path exists in `perforce.rs::sync_workspace`
  but not wired to UI. Useful when v0.6 adds non-Unreal review flows
  (storyboard reviewers don't open Unreal).
- **DPAPI-encrypted state** — plaintext for v0.5 (see decision #5).
- **Multi-monitor tray placement** — uses OS default.
- **Phone-home for invite revocation** — invite expiration is local-only;
  server-side revocation works (delete the Perforce user) but Companion
  won't know until next reconnect.

## Known gaps + things to test

1. **`cargo tauri build` validation:** the production .msi build runs at
   the end of Session 2. Confirm it produces a working installer in
   `src-tauri/target/release/bundle/msi/` and that installing it on a
   clean Win11 VM works.
2. **UAC trampoline:** the PowerShell `Start-Process -Verb RunAs` invocation
   in `tailscale.rs::install_via_msi` has been exercised in isolation but
   not yet from inside a packaged Companion build. The risk is that some
   Windows versions reject elevated `Start-Process` from a non-elevated
   parent if the parent isn't trusted; if that happens, add a manifest
   `requestedExecutionLevel="requireAdministrator"` for the installer
   bootstrapper stage.
3. **Smoke invite flow:** end-to-end test (paste smoke invite → land on
   credentials panel) requires running the packaged build and watching
   the chain fire. The reusable Tailscale auth key in
   `.scratch/perforce_session/test_invite_for_session_2.txt` survives
   multiple uses, so this is safe to re-run.
4. **Tray icon color polling:** the tray icon is built but does NOT yet
   poll Tailscale + Perforce status to color itself green/yellow/red
   every 30 s as the kickoff specifies. Phase 6 polish item.
5. **Deep-link registration:** registered via `tauri-plugin-deep-link`
   config — should be picked up automatically by the MSI installer.
   Verify by clicking a `spaceshop-companion://invite/...` URL after
   installing.

## v0.5 build artifacts (Session 2)

`npm run tauri build` produced both installer formats successfully on
2026-05-20:

```
src-tauri/target/release/bundle/msi/Spaceshop Companion_0.5.0_x64_en-US.msi   (37 MB)
src-tauri/target/release/bundle/nsis/Spaceshop Companion_0.5.0_x64-setup.exe  (34 MB)
```

The MSI bundles:
- `spaceshop-companion.exe` (Companion itself, ~5 MB stripped)
- `binaries/p4.exe` (Perforce CLI, 9.4 MB)
- `binaries/tailscale.msi` (Tailscale Windows installer 1.98.2, 35 MB)
- Tauri's `WebView2Loader.dll` bootstrap

Build time on the dev workstation (4090, 32-core): ~3 minutes cold,
sub-2-minute incremental. WiX 3.14 and NSIS 3.11 are downloaded by
Tauri on first build and cached in `%LOCALAPPDATA%/tauri/`.

## How to rebuild the .msi

See `BUILDING.md`.

## How to smoke-test the .msi

1. Copy `Spaceshop Companion_0.5.0_x64_en-US.msi` to a clean Win11 VM
   (or just install it on the dev workstation).
2. Open the MSI. SmartScreen warns → "More info" → "Run anyway".
3. After install, Companion opens automatically.
4. Paste the smoke invite from
   `C:\LOCAL_PROJECTS\Spaceshop_Perforce\SPACESHOP TOOLS\.scratch\perforce_session\test_invite_for_session_2.txt`
   into the textbox. Click **Continue**.
5. Confirm the project (`Session 2 smoke — smoke-v4`) and accept the
   default folder (or pick another). Click **Connect**.
6. On the UAC prompt that appears while Tailscale installs, click **Yes**.
7. The 7-step chain runs. Expected outcome: lands on the credentials
   panel showing `100.82.0.8:1666 / sarah-test / sarah-test-session-2-smoke`
   with 3 files synced from `//smoke-v4/main`.
8. (Optional) Open the project folder — you should see the
   `project.json`, `.p4ignore`, and `shots/index.json` from the
   throwaway-project migration.

The smoke invite's Tailscale auth key is **reusable** (regenerated at
end of Session 51 to be non-ephemeral), so this smoke test can be
re-run as many times as needed. Per-contractor production invites
should still use ephemeral keys.
