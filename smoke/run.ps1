# Spaceshop Companion — Windows Sandbox smoke test driver
#
# Runs inside a fresh Windows Sandbox VM via smoke.wsb's LogonCommand.
# Installs the first *.msi found in C:\smoke\ silently, verifies the
# installed exe is on disk, launches it, and leaves a PASS/FAIL marker
# in C:\smoke\transcript.txt so the host can grep the result.
#
# Exit codes are advisory — Windows Sandbox closes when the user closes
# the window, so the host reads transcript.txt rather than waiting on $LASTEXITCODE.

$ErrorActionPreference = 'Continue'  # we handle errors ourselves so the transcript captures them

$transcriptPath = 'C:\smoke\transcript.txt'
$msiLogPath     = 'C:\smoke\msi.log'
$installedExe   = 'C:\Program Files\Spaceshop Companion\spaceshop-companion.exe'

# Start fresh — Stop-Transcript first in case a previous run left one open
try { Stop-Transcript | Out-Null } catch { }
Start-Transcript -Path $transcriptPath -Force | Out-Null

$pass = $false
$failReason = $null

try {
    Write-Host "=== Spaceshop Companion smoke test ==="
    Write-Host "Started: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
    Write-Host "Host: $env:COMPUTERNAME"
    Write-Host ""

    # --- 1. Find the MSI ---
    Write-Host "[1/5] Locating MSI in C:\smoke\ ..."
    $msi = Get-ChildItem -Path 'C:\smoke\' -Filter '*.msi' -File | Select-Object -First 1
    if (-not $msi) {
        throw "No *.msi found in C:\smoke\. Copy the built MSI into the host's smoke\ folder and re-launch smoke.wsb."
    }
    Write-Host "      Found: $($msi.FullName) ($([math]::Round($msi.Length/1MB, 1)) MB)"
    Write-Host ""

    # --- 2. Silent install ---
    Write-Host "[2/5] Running msiexec /i ... /qn /l*v $msiLogPath"
    $msiArgs = @('/i', "`"$($msi.FullName)`"", '/qn', '/l*v', "`"$msiLogPath`"")
    $proc = Start-Process -FilePath 'msiexec.exe' -ArgumentList $msiArgs -Wait -PassThru -NoNewWindow
    $exit = $proc.ExitCode
    Write-Host "      msiexec exit code: $exit"
    if ($exit -ne 0) {
        throw "msiexec failed with exit code $exit. See $msiLogPath for details."
    }
    Write-Host ""

    # --- 3. Verify installed exe ---
    Write-Host "[3/5] Verifying installed exe at $installedExe"
    if (-not (Test-Path -LiteralPath $installedExe)) {
        # Dump Program Files\Spaceshop Companion if it exists, to aid debugging
        $instDir = 'C:\Program Files\Spaceshop Companion'
        if (Test-Path -LiteralPath $instDir) {
            Write-Host "      Install dir exists but exe missing. Contents:"
            Get-ChildItem -LiteralPath $instDir -Recurse | ForEach-Object { Write-Host "        $($_.FullName)" }
        } else {
            Write-Host "      Install dir $instDir does NOT exist."
        }
        throw "Installed exe not found at $installedExe."
    }
    $exeInfo = Get-Item -LiteralPath $installedExe
    Write-Host "      OK — $($exeInfo.Length) bytes, version $($exeInfo.VersionInfo.FileVersion)"
    Write-Host ""

    # --- 4. Launch the exe ---
    Write-Host "[4/5] Launching Spaceshop Companion ..."
    $launched = $null
    try {
        $launched = Start-Process -FilePath $installedExe -PassThru -ErrorAction Stop
        Write-Host "      Launched PID $($launched.Id)"
    } catch {
        throw "Start-Process failed: $($_.Exception.Message)"
    }
    Write-Host ""

    # --- 5. Wait 12s, verify process still alive ---
    Write-Host "[5/5] Waiting 12s for first-run init ..."
    Start-Sleep -Seconds 12

    # Re-query the process — if it crashed immediately, the original handle is stale.
    $stillRunning = Get-Process -Id $launched.Id -ErrorAction SilentlyContinue
    if (-not $stillRunning) {
        throw "Companion process (PID $($launched.Id)) exited within 12s of launch — likely crash on startup."
    }
    Write-Host "      OK — PID $($launched.Id) still running after 12s."
    Write-Host "      (No automated UI assertion yet — v0.6 will add WebView2 WebDriver.)"
    Write-Host ""

    $pass = $true
} catch {
    $failReason = $_.Exception.Message
    Write-Host ""
    Write-Host "!!! SMOKE TEST FAILED !!!"
    Write-Host "    Reason: $failReason"
    Write-Host ""
}

# Final marker — host greps for this
Write-Host ""
Write-Host "=== RESULT ==="
if ($pass) {
    Write-Host "SMOKE_TEST: PASS"
} else {
    Write-Host "SMOKE_TEST: FAIL"
    if ($failReason) { Write-Host "FAIL_REASON: $failReason" }
}
Write-Host "Finished: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
Write-Host ""
Write-Host "Sandbox will stay open. Close the window when done reviewing."
Write-Host "Transcript: $transcriptPath"
Write-Host "MSI log:    $msiLogPath"

Stop-Transcript | Out-Null
