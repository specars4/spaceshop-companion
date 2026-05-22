# Releasing Spaceshop Companion

Companion is distributed as an unsigned Windows installer with built-in
self-update via [Tauri's updater plugin](https://v2.tauri.app/plugin/updater/).
This doc captures the exact ceremony for cutting a release. Follow it
verbatim until it's wrong — then fix the doc.

## TL;DR

1. Bump `version` in three files: `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `package.json`
2. Update `SESSIONS.md` with what changed in this release
3. Run `npm run tauri build` with the signing key env var set
4. Create a GitHub Release tagged `v<version>` and upload the `.msi` + `latest.json`
5. Contractors' Companion apps see the update on next launch within 8 seconds

## Prerequisites (one-time)

### Signing key
The Ed25519 signing keypair was generated during the v0.5.1 build:
- **Public key** is committed to the repo inside `src-tauri/tauri.conf.json`
  under `plugins.updater.pubkey`. Safe to share.
- **Private key** lives at `.keys/companion-updater.key` after generation —
  this directory is `.gitignore`'d. **Move the file immediately into a
  password manager** (1Password, Bitwarden, Keychain) and delete the local
  copy. Loss = permanent inability to ship updates.

To regenerate (only if the private key is lost — every existing Companion
install will then refuse to update and must be reinstalled by hand from a
new .msi):
```powershell
npx tauri signer generate -w .keys/companion-updater.key --ci -p ""
```
Replace the `pubkey` in `tauri.conf.json` with the new public key.

### GitHub repo
First time:
1. Create the repo at `https://github.com/<your-org>/spaceshop-companion`.
   Visibility: private is fine (GitHub Releases binaries are still public).
2. Push the source up.
3. Update the `endpoints` URL in `src-tauri/tauri.conf.json` to use the real
   org/repo name in place of `SPACESHOP_GH_ORG`:
   ```
   "endpoints": [
     "https://github.com/<your-org>/spaceshop-companion/releases/latest/download/latest.json"
   ]
   ```

This URL pattern uses GitHub's "latest release" redirect — no need to
update it per release.

## Per-release ceremony

### 1. Bump versions (in lock-step, all three files)

Pick the new version (semver). For routine fixes: `0.5.1 → 0.5.2`. For
features: `0.5.x → 0.6.0`.

| File | Field |
|---|---|
| `src-tauri/Cargo.toml` | `[package].version` |
| `src-tauri/tauri.conf.json` | `version` (top-level) |
| `package.json` | `version` |

### 2. Update `SESSIONS.md`

Add a "v0.x.y — YYYY-MM-DD" section at the top describing what shipped.
Future agents and humans read it.

### 3. Build the signed installer

Set the signing-key env vars and build. Treat the private-key value like a
password — don't paste it in chat logs.

```powershell
# Pull the private key out of your password manager and paste it here:
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content "$HOME\private\spaceshop-companion-updater.key" -Raw
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""   # empty if you used --no-password during keygen

# Build
cd C:\LOCAL_PROJECTS\spaceshop-companion
npm run tauri build
```

Build outputs (under `src-tauri/target/release/bundle/`):
- `msi/Spaceshop Companion_<version>_x64_en-US.msi` — the installer
- `msi/Spaceshop Companion_<version>_x64_en-US.msi.sig` — the signature
  (Tauri produces this when the env vars are set)

Tauri's NSIS output (`nsis/Spaceshop Companion_<version>_x64-setup.exe`) is
also produced but is a secondary artifact — the MSI is canonical for
auto-update.

### 4. Make the `latest.json` manifest

The updater plugin needs a JSON file describing the new release. Create
`latest.json` next to the .msi:

```json
{
  "version": "0.5.2",
  "notes": "What changed in plain language — shown to the user in the update banner.",
  "pub_date": "2026-05-21T18:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "<paste the contents of the .msi.sig file here>",
      "url": "https://github.com/<your-org>/spaceshop-companion/releases/download/v0.5.2/Spaceshop_Companion_0.5.2_x64_en-US.msi"
    }
  }
}
```

Get the signature by running:
```powershell
Get-Content "src-tauri\target\release\bundle\msi\Spaceshop Companion_<version>_x64_en-US.msi.sig" -Raw
```

### 5. Upload to GitHub Releases

Using the GitHub UI:
1. Go to `https://github.com/<your-org>/spaceshop-companion/releases/new`
2. Tag: `v<version>` (e.g. `v0.5.2`)
3. Title: `Companion v<version>`
4. Body: paste the same `notes` text as in `latest.json` (for the human reader)
5. Attach two files: the `.msi` and `latest.json`
6. Mark as "Latest release" — this updates the `/releases/latest/` redirect
7. Publish

Or via `gh` CLI:
```powershell
gh release create v0.5.2 `
  --title "Companion v0.5.2" `
  --notes-file release-notes.md `
  "src-tauri\target\release\bundle\msi\Spaceshop Companion_0.5.2_x64_en-US.msi" `
  "latest.json"
```

### 6. Verify

On any installed Companion (older version), wait ~8 seconds after launch.
The update banner should appear: "Update available — v0.5.1 → v0.5.2".
Click Install. The app downloads + installs + restarts. Confirm the
version in the title bar / About dialog is the new one.

## Rollback

If v0.7.0 turns out to be broken, you have two options:

**(a) Ship v0.7.1 that reverts the bad change.** Cleanest — the version
chain stays monotonic. Tauri's updater won't move backwards.

**(b) Delete the v0.7.0 GitHub Release.** Existing v0.7.0 installs stay on
v0.7.0, but new installs (downloaded from `/releases/latest/`) fall back
to v0.6. Use this only if v0.7.0 is genuinely catastrophic. You'll still
want a v0.7.1 fix shortly after.

There is **no auto-downgrade**. Once a user installs v0.7.0 they only
move forward via the next release.

## Migration plan — GitHub → NAS-hosted updates

Long-term we may want updates flowing through the studio's NAS over
Tailscale instead of GitHub. The reasons would be: keeping artifacts
private without GitHub auth, lower latency for contractors already on the
tailnet, full studio control.

When ready:

1. Set up an HTTPS endpoint on the NAS — nginx container in DSM, serving
   files from `/volume1/perforce/../companion-updates/` (separate from
   p4d's bind mount). Endpoint URL like `https://100.82.0.8/companion-updates/latest.json`.
2. Ship a "switchover" release (call it v1.0.0): bundle that release with
   `endpoints: ["NAS_URL"]` in `tauri.conf.json`.
   - Contractor on v0.9.x polls GitHub, sees v1.0.0, downloads from GitHub,
     installs.
   - v1.0.0 starts polling the NAS for everything after that.
3. **Don't delete GitHub Releases.** Keep it as a permanent fallback — costs
   nothing. Future releases can configure `endpoints: ["NAS_URL", "GITHUB_URL"]`
   to try NAS first and fall back to GitHub if the contractor is off
   Tailscale.
4. If you ever DO want to delete GitHub: wait until you're confident every
   contractor has updated past v1.0.0 (just ask in Slack — small team, no
   telemetry needed). Then delete the older releases.

## Things that go wrong

| Symptom | Likely cause | Fix |
|---|---|---|
| `tauri build` produces no `.msi.sig` | Env var not set or empty | Confirm `$env:TAURI_SIGNING_PRIVATE_KEY` has the full key including the comment line |
| Updater says "signature does not match" | The `latest.json` signature doesn't match the .msi, OR the public key in `tauri.conf.json` doesn't match the private key used for signing | Re-paste the `.msi.sig` content into `latest.json`. If still failing, you may have signed with a different key — check which key file you used. |
| Banner never appears | Endpoint URL 404, or the running version is already at or above `latest.json`'s version | Open the endpoint URL in a browser; confirm it returns the latest.json. Check version numbers. |
| `download_and_install` fails partway | Network blip or antivirus interception | Click Install again. If persistent, send the user a fresh .msi to install manually. |
| Contractor on Win10 says SmartScreen blocks the update install | Update installs the bundled MSI which is unsigned | Same workaround as initial install: "More info" → "Run anyway". Or: pay for an Authenticode cert (see BUILDING.md). |

## Channel strategy

For now everything ships to one channel = "stable" = the GitHub `latest`
release.

If we ever want a beta channel (for sarah-test and arsen to try a build
before contractors get it), Tauri's updater supports multiple endpoints
keyed by tag. Set it up when we have a second tester. Documented in
[Tauri's updater docs](https://v2.tauri.app/plugin/updater/).
