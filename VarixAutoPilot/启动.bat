@echo off
REM ============================================================
REM  VarixAutoPilot launcher
REM  ASCII-only + CRLF on purpose: cmd reads this file as GBK;
REM  any non-ASCII byte becomes mojibake.
REM ============================================================
setlocal
set HERE=%~dp0
set NODE=C:\Users\varia\.workbuddy\binaries\node\versions\22.22.2-3\node.exe
set REPO=D:\2\14\-Un-Real-0d23d9ux-Engine-main

if not exist "%NODE%" (
  echo [ERR] node.exe not found: %NODE%
  echo       Check the path, or edit this .bat.
  pause
  exit /b 1
)

REM --- check CDP port 9222 ---
curl -s -m 3 "http://127.0.0.1:9222/json/version" >nul 2>&1
if errorlevel 1 (
  echo [WARN] Port 9222 is closed.
  echo        WorkBuddy must be started with:
  echo          --remote-debugging-port=9222 --remote-debugging-address=127.0.0.1
  echo        Panel will start anyway, but cannot read conversations.
  echo.
)

REM --- port 8768 already in use? ---
curl -s -m 3 "http://127.0.0.1:8768/api/snapshot" >nul 2>&1
if not errorlevel 1 (
  echo [OK] Autpilot already running at http://127.0.0.1:8768/
  start "" "http://127.0.0.1:8768/"
  exit /b 0
)

echo [..] Starting VarixAutoPilot ...
set NODE_PATH=%REPO%\node_modules
start "" "http://127.0.0.1:8768/"
"%NODE%" "%HERE%server.mjs"

endlocal
