# Building Spaceshop Companion

## Prerequisites

One-time setup on a Windows 10/11 build machine:

1. **Rust toolchain** (stable, x86_64-pc-windows-msvc target):
   ```powershell
   curl -sSf -o rustup-init.exe https://win.rustup.rs/x86_64
   .\rustup-init.exe -y --default-toolchain stable --profile default
   ```
   Adds `%USERPROFILE%\.cargo\bin` to PATH; restart your shell.

2. **Node.js 20+** (for the frontend build). Install from https://nodejs.org/
   or via `winget install OpenJS.NodeJS.LTS`.

3. **Microsoft C++ Build Tools** (MSVC + Windows SDK). Either:
   - Visual Studio 2022 Community (Desktop development with C++), OR
   - "Build Tools for Visual Studio 2022" + the "Desktop development with C++"
     workload.

4. **WebView2 Runtime** — preinstalled on Windows 11; for Windows 10 install
   from https://developer.microsoft.com/microsoft-edge/webview2/.

5. **Tauri's bundled tools** (WiX for .msi, NSIS for .exe) are downloaded
   automatically the first time `cargo tauri build` runs.

## First-time setup of this repo

```powershell
cd <COMPANION_REPO_ROOT>     # wherever you cloned this repo
npm install
```

(The first `cargo` invocation will pull all Rust dependencies; expect
the first build to take ~15-20 minutes.)

## Day-to-day dev loop

```powershell
npm run tauri dev
```

Hot-reload for the frontend, Rust recompiles on save. The dev window
behaves like the packaged app except:
- No URL handler registration (deep-link plugin needs the installer)
- Resources are read from `src-tauri/binaries/` directly instead of
  the installed `resources/` folder

## Producing the .msi installer

```powershell
npm run tauri build
```

Outputs:
- `src-tauri/target/release/bundle/msi/Spaceshop Companion_<version>_x64_en-US.msi`
- `src-tauri/target/release/bundle/nsis/Spaceshop Companion_<version>_x64-setup.exe`

The MSI is what to send to contractors. The NSIS .exe is a smaller
alternative if .msi distribution becomes a problem (some corporate IT
policies block MSI-not-from-allowlist).

## Smoke testing

Every release should be smoke-tested in a clean Windows Sandbox before
publishing. The harness lives in `smoke/`:

1. Copy the freshly built MSI into `smoke/`.
2. Double-click `smoke/smoke.wsb`.
3. Read `smoke/transcript.txt` — look for `SMOKE_TEST: PASS`.

This catches the class of bugs where the MSI builds on the dev machine
but fails on a clean install (missing dependencies, hardcoded dev-profile
paths, WiX config typos). See `smoke/README.md` for full details, host
setup, and limitations (reboot-required installs and deep-link
registration must be tested in a real VM).

## Updating the bundled p4.exe

Source of truth: `<SPACESHOP_TOOLS_ROOT>/tools/perforce/bin/p4.exe`
(provenance + SHA-256 in that folder's `README.md`).

```powershell
Copy-Item "<SPACESHOP_TOOLS_ROOT>\tools\perforce\bin\p4.exe" `
  "<COMPANION_REPO_ROOT>\src-tauri\binaries\p4.exe" -Force
```

Then rebuild.

## Updating the bundled Tailscale MSI

When Tailscale releases a new version:

```powershell
$version = "1.99.0"  # check https://pkgs.tailscale.com/stable/
$url = "https://pkgs.tailscale.com/stable/tailscale-setup-$version-amd64.msi"
Invoke-WebRequest $url -OutFile "<COMPANION_REPO_ROOT>\src-tauri\binaries\tailscale.msi"
```

Then bump the Companion `version` in `src-tauri/Cargo.toml` and
`src-tauri/tauri.conf.json`, and rebuild. The new MSI silently upgrades
any older Tailscale install on the contractor's machine.

## Versioning

Three places to bump in lock-step:
- `src-tauri/Cargo.toml` → `version = "x.y.z"`
- `src-tauri/tauri.conf.json` → `"version": "x.y.z"`
- `package.json` → `"version": "x.y.z"`

## Signing (deferred to v0.6+)

The .msi is unsigned. SmartScreen will show "Windows protected your PC"
on first install. Contractors click "More info" → "Run anyway". The
contractor-onboarding email template covers this — see
`docs/CONTRACTOR_ONBOARDING_TEMPLATE.md`.

When we're ready to sign:
- Get an Authenticode certificate (DigiCert, Sectigo, etc — ~$200/yr)
- Configure `tauri.conf.json` `bundle.windows.signCommand` to invoke
  `signtool sign`
- Re-bundle. SmartScreen warning goes away after Microsoft has seen
  enough installs of a signed binary (usually a few hundred).
