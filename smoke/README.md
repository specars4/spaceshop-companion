# Spaceshop Companion — Windows Sandbox smoke test

A one-double-click way to verify that a freshly built MSI installs cleanly
and the app actually launches on a pristine Windows machine. Uses Windows
Sandbox, which spins up a throwaway VM in ~10-20s and resets to a clean
state every launch.

## One-time host setup

Windows Sandbox is built into **Windows 11 Pro / Enterprise / Education**
(also Win 10 Pro 1903+). It is **not** available on Home editions — if
Arsen or a contractor is on Home, see the "Alternatives" section below.

From an **elevated PowerShell** (Run as Administrator):

```powershell
Enable-WindowsOptionalFeature -FeatureName "Containers-DisposableClientVM" -All -Online
```

Reboot. Confirm by typing "Windows Sandbox" into the Start menu — you
should see the app appear.

## Smoke-test workflow

Every time you cut a new release:

1. **Build the MSI** in the repo root:
   ```powershell
   npm run tauri build
   ```
2. **Copy the MSI** into this `smoke/` folder. Only one `*.msi` should be
   present — the script picks the first one it finds.
   ```powershell
   Copy-Item ".\src-tauri\target\release\bundle\msi\Spaceshop Companion_*.msi" .\smoke\
   ```
3. **Double-click `smoke.wsb`**. Windows Sandbox boots, the host
   `smoke\` folder mounts at `C:\smoke\` inside the VM (read+write), and
   `run.ps1` fires automatically.
4. **Watch the sandbox window** — `run.ps1` prints progress to a
   PowerShell console as it runs. After ~30s you'll see either
   `SMOKE_TEST: PASS` or `SMOKE_TEST: FAIL` at the bottom.
5. **Read the artifacts** back on the host (since the mount is read+write,
   they're already there):
   - `smoke/transcript.txt` — full PowerShell transcript of the run
   - `smoke/msi.log` — `msiexec` verbose log (huge but invaluable on install failure)

## What PASS / FAIL look like

### PASS

End of `smoke/transcript.txt`:
```
=== RESULT ===
SMOKE_TEST: PASS
Finished: 2026-05-22 14:33:17
```

This means: MSI installed silently with exit code 0, the installed exe
exists at `C:\Program Files\Spaceshop Companion\spaceshop-companion.exe`,
it launched without throwing, and the process was still alive 12s later.

### FAIL

End of `smoke/transcript.txt`:
```
!!! SMOKE TEST FAILED !!!
    Reason: msiexec failed with exit code 1603. See C:\smoke\msi.log for details.

=== RESULT ===
SMOKE_TEST: FAIL
FAIL_REASON: msiexec failed with exit code 1603. See C:\smoke\msi.log for details.
```

Common failure modes:
- **No MSI in the folder** — you forgot step 2 above. Copy the MSI in
  and double-click `smoke.wsb` again.
- **msiexec exit 1603 / 1625** — install error. `smoke/msi.log` will have
  the WiX-level reason (missing dependency, file-in-use, permission).
- **Installed exe not found** — MSI claimed success but didn't drop the
  exe where expected. Almost always a WiX config bug in `tauri.conf.json`.
- **Process exited within 12s of launch** — Companion crashed on
  startup. Re-run locally with a debugger or check Windows Event Viewer
  inside the sandbox (it lives ~5 min after the script finishes, until
  you close the window).

To grep results from the host quickly:
```powershell
Select-String -Path .\smoke\transcript.txt -Pattern '^SMOKE_TEST:'
```

## When to use this (and when not to)

**Use the sandbox smoke test for every release before publishing**, i.e.
every v0.5.x and v0.6.x bump. It catches the embarrassing class of bugs
where the MSI builds fine on the dev machine but fails on a clean machine
because of an undeclared dependency, a hardcoded path under the dev
profile, or a Tauri config typo.

**It is not sufficient for:**
- **Anything that requires a reboot** — Tailscale's network stack install,
  certain WebView2 first-run paths. Use a real Hyper-V VM (or a spare
  laptop) for those.
- **Deep-link / URL-handler registration** — Windows Sandbox blocks some
  shell integrations; the deep-link plugin may register but not respond
  to `spaceshop-companion://` invocations from the host browser. Test
  deep links on a real machine.
- **WebView2 UI interactions** — v0.6 will add a WebView2 WebDriver
  automation layer that drives the rendered UI to verify the invite flow
  end-to-end. Until then, "process still alive at +12s" is the bar.

## Alternatives for Win Home machines

If you must smoke-test on a Win Home host, use a Hyper-V or VirtualBox VM
with a fresh Windows 11 install, restore a "clean baseline" snapshot
before each test, run `run.ps1` manually from inside, and revert after.
Slower (minutes vs seconds) but works everywhere.
