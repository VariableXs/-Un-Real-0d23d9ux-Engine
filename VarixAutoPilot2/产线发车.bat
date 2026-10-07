@echo off
REM ============================================================
REM  VarixAutoPilot CLAIM tower - one-click launch
REM
REM  Production line = CLAIM mode: workers pull tasks from
REM  VTaskBoard (port 8767). The first prompt (Variable's verbatim
REM  instruction + ops notes) lives INSIDE claim_tower.py as
REM  PROTOCOL_TEMPLATE. Worker IDs W001..W018 auto-increment,
REM  one unique ID per new conversation.
REM
REM  Step 1: probe (read-only health check). Abort if it fails.
REM  Step 2: create 18 worker conversations + bootstrap (workspace
REM          + 4 files + 10 skills via slash panel) + send prompt.
REM  Step 3: settle 5 min untouched, then health-check all 18 workers;
REM          any that never started on its own (state file still the
REM          tower's placeholder) gets archived + rebuilt as a NEW
REM          conversation with the full protocol.
REM  Protocol embeds the owner's directive: ALL features revolve
REM  around the Rust kernel and are implemented in Rust only.
REM  Legacy TypeScript (~1400 files under src/) is fully migrated
REM  to Rust: tasks landing in TS areas migrate the involved TS
REM  modules to Rust first, then build on the Rust implementation.
REM  Replaced TS files may be deleted after the Rust version
REM  passes acceptance and no other references remain.
REM
REM  Account switch: abnormal/banned TreeCode accounts are skipped and
REM  the next candidate is tried until one works. After a switch, if
REM  >=2/3 of the 18 conversations survived with real worker state they
REM  are KEPT (nudged back to work + settle health check) instead of a
REM  full 18-conversation rebuild.
REM
REM  24h guard: if the tower crashes (any exit code other than 0/2)
REM  this bat restarts it after 60s. Exit 0 = graceful stop / wrap-up,
REM  exit 2 = halted (no usable credit account) - no auto restart.
REM
REM  Requires: WorkBuddy running with CDP port 9222.
REM  If probe fails, run open-port.bat first.
REM
REM  Keep this file ASCII only (cmd reads bat as GBK).
REM ============================================================
setlocal
set "AP_DIR=D:\2\14\-Un-Real-0d23d9ux-Engine-main\VarixAutoPilot2"
set "PY=C:\Users\varia\.workbuddy\binaries\python\versions\3.13.12\python.exe"

if not exist "%PY%" (
  echo [ERR] python.exe not found: %PY%
  pause
  exit /b 1
)

echo === [1/2] Health probe (read-only) ===
"%PY%" "%AP_DIR%\tools\claim_tower.py" --probe
if errorlevel 1 (
  echo.
  echo [ABORT] probe failed - fix the issues above, then rerun.
  pause
  exit /b 1
)

echo.
echo === [2/2] Launch 18-worker claim production line (24h auto-restart) ===
:loop
"%PY%" "%AP_DIR%\tools\claim_tower.py" --start 18 --watch --settle-min 5 %*
set "EC=%errorlevel%"
if "%EC%"=="0" (
  echo.
  echo Tower exited gracefully (wrap-up or manual Ctrl+C). Workers keep running.
  pause
  goto :eof
)
if "%EC%"=="2" (
  echo.
  echo Tower halted: no usable credit account. Manual attention required.
  pause
  goto :eof
)
echo.
echo [24h guard] Tower crashed (exit code %EC%) - restarting in 60s...
timeout /t 60 /nobreak >nul
goto loop
