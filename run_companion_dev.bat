@echo off
REM ──────────────────────────────────────────────────────────────────────────
REM  Spaceshop Companion — dev launcher.
REM
REM  Workshop-equivalent iteration loop. Frontend changes hot-reload
REM  instantly; Rust changes recompile+restart.
REM
REM  Dev-build link time on Windows MSVC is the slowest stage — typically
REM  60-180s for a clean relink of the ~28 MB Tauri exe. Cargo.toml's
REM  profile.dev disables debuginfo for third-party deps which cuts
REM  link time roughly in half. (LLD-link was tried; rust-lld's
REM  Windows wrapper rejected our build-script chain — abandoned.)
REM
REM  Every stage prints a timestamp so you can tell which step is slow.
REM  Full output is also tee'd to dev-log.txt for after-the-fact
REM  diagnosis if the window closes on you.
REM ──────────────────────────────────────────────────────────────────────────

setlocal EnableDelayedExpansion

REM Ensure cargo + node are reachable
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

cd /d "%~dp0"

set "LOGFILE=%~dp0dev-log.txt"

call :stage "Launching Companion dev mode"
call :stage "Log file: %LOGFILE%"
echo. > "%LOGFILE%"

REM Sanity check
where cargo >nul 2>nul || ( call :stage "ERROR: cargo not found on PATH. Install Rust via rustup first." & pause & exit /b 1 )
where node  >nul 2>nul || ( call :stage "ERROR: node not found on PATH. Install Node 20+ first." & pause & exit /b 1 )

REM npm install if missing
if not exist "node_modules" (
  call :stage "Installing npm dependencies (one-time, ~30s)..."
  call npm install >> "%LOGFILE%" 2>&1 || ( call :stage "ERROR: npm install failed. See %LOGFILE%" & pause & exit /b 1 )
)

REM Hint at port-1420 collision before we even try (Vite binds 1420)
netstat -ano -p tcp ^| findstr ":1420 " ^| findstr LISTENING >nul 2>nul && (
  call :stage "WARNING: port 1420 is already in use — Vite will fail to bind."
  call :stage "         Most likely another tauri dev is running. Find + kill it, then re-run."
  netstat -ano -p tcp | findstr ":1420 " | findstr LISTENING
  pause
  exit /b 1
)

call :stage "Starting Vite (frontend) + cargo (Rust) — output below + in %LOGFILE%"
call :stage "First launch on a clean cache: ~30-90s. Incremental: ~5-15s."
call :stage "When the Companion window appears, dev mode is live."
echo.
echo --- npm run tauri dev ---
echo.

REM Tee npm output to both the console and the logfile.
REM PowerShell's Tee-Object handles this cleanly without needing extra deps.
powershell -NoProfile -Command "& { npm run tauri dev 2>&1 | Tee-Object -FilePath '%LOGFILE%' -Append }"

set "EXITCODE=%ERRORLEVEL%"
call :stage "Dev mode exited (code %EXITCODE%). Log saved to %LOGFILE%."
pause
exit /b %EXITCODE%

REM ── subroutines ──────────────────────────────────────────────────────────

:stage
  echo [%TIME%] %~1
  goto :eof
