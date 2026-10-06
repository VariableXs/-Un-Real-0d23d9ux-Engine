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
echo === [2/2] Launch 18-worker claim production line ===
"%PY%" "%AP_DIR%\tools\claim_tower.py" --start 18 --watch %*

echo.
echo Tower stopped. Workers keep running; rerun this to resume.
pause
