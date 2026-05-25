# Spaceshop Companion

The contractor onboarding app for Spaceshop Studios projects. A
contractor pastes an invite code, picks a folder, and Companion sets up
the connection + workspace + initial download so they can open the
project in Unreal with three copy-paste values.

**Status:** v0.6.0 — Windows only, unsigned MSI, self-updating via GitHub Releases.
**Built with:** Tauri 2.x · React 19 · TypeScript 5.8

## Quick links

- [`SESSIONS.md`](SESSIONS.md) — chronological log of build sessions
- [`BACKLOG.md`](BACKLOG.md) — queued work for next sessions
- [`docs/COMPANION_HANDOFF.md`](docs/COMPANION_HANDOFF.md) — what was
  built, technical decisions, gaps
- [`docs/RELEASING.md`](docs/RELEASING.md) — how to cut + publish a new release
- [`docs/BUILDING.md`](docs/BUILDING.md) — toolchain + `npm run tauri build`
- [`docs/USER_GUIDE.md`](docs/USER_GUIDE.md) — what contractors see
- [`docs/CONTRACTOR_ONBOARDING_TEMPLATE.md`](docs/CONTRACTOR_ONBOARDING_TEMPLATE.md)
  — the message Arsen sends with the .msi
- [`docs/INVITE_FORMAT.md`](docs/INVITE_FORMAT.md) — the v=1 invite
  schema (mirror of the canonical spec in the SPACESHOP TOOLS repo)

## Run in dev

```powershell
npm install
npm run tauri dev
```

## Produce installers

```powershell
npm run tauri build
```

Outputs:
- `src-tauri/target/release/bundle/msi/Spaceshop Companion_<version>_x64_en-US.msi`
- `src-tauri/target/release/bundle/nsis/Spaceshop Companion_<version>_x64-setup.exe`

## Scope

Companion is **bootstrap-only**. Daily Perforce work happens inside
Unreal Engine (file checkouts, submits) or inside Workshop's
PERFORCE tab (Arsen's admin surface). See `COMPANION_HANDOFF.md` for
the full division of responsibilities.
