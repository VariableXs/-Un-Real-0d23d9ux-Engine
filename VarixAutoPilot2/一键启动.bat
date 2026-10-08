@echo off
REM ============================================================
REM  VarixAutoPilot - one-click: port + app
REM
REM  Flow:
REM    1. check / open the debug port (refuses to kill WorkBuddy)
REM    2. if the port is not ready, tell the user exactly what to do
REM    3. once ready, launch VarixAutoPilot.exe
REM
REM  Keep ASCII + CRLF (cmd reads bat as GBK).
REM  Quote any path containing hyphens: "set X=...\path-with-hyphen"
REM ============================================================
setlocal

set "AP_DIR=D:\2\14\-Un-Real-0d23d9ux-Engine-main\VarixAutoPilot2"
set "PY=C:\Users\varia\.workbuddy\binaries\python\versions\3.13.12\python.exe"

REM --- is the DevTools endpoint already alive? ---
curl -s -m 3 "http://127.0.0.1:9222/json/version" >nul 2>&1
if not errorlevel 1 (
  echo [OK] Port 9222 is ready.
  start "" "%AP_DIR%\VarixAutoPilot.exe"
  endlocal
  exit /b 0
)

echo ============================================================
echo  Port 9222 is NOT ready.
echo ============================================================
echo
echo  The debug port is decided when WorkBuddy STARTS.
echo  An already-running WorkBuddy will never open it.
echo
echo  Your options:
echo
echo    A - Quickest, no interruption:
echo       Quit WorkBuddy yourself: right-click its tray icon, Exit.
echo       Then run the open-port script on the Desktop.
echo
echo    B - Let the script do it (interrupts running conversations):
echo       Close this window, then double-click:
echo         the open-port script --kill
echo
echo  Opening VarixAutoPilot anyway - it will show exactly what is missing.
echo.
echo.

start "" "%AP_DIR%\VarixAutoPilot.exe"
endlocal
