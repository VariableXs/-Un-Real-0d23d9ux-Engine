@echo off
REM ============================================================
REM  VarixAutoPilot 2 - launcher
REM  ASCII + CRLF on purpose: cmd reads this file as GBK.
REM
REM  Placed on the Desktop so the app is one double-click away.
REM  Checks the CDP port first, so a closed port produces a clear
REM  message instead of a silent "nothing happened".
REM
REM  BUG NOTE (hit while testing this): the repo path contains hyphens
REM  ("-Un-Real-..."). In a batch file you MUST quote such a path when
REM  assigning it, otherwise cmd treats the text after the hyphen as a
REM  command and dies with a syntax error. Do not "simplify" the quotes
REM  back out.
REM ============================================================
setlocal

set "AP_DIR=D:\2\14\-Un-Real-0d23d9ux-Engine-main\VarixAutoPilot2"
set "AP_EXE=%AP_DIR%\VarixAutoPilot.exe"

if not exist "%AP_EXE%" (
  echo.
  echo [ERR] VarixAutoPilot.exe not found:
  echo        %AP_EXE%
  echo.
  echo        Rebuild it with:
  echo          cd "%AP_DIR%\src-tauri"
  echo          cargo tauri build --no-bundle
  echo        then copy src-tauri\target\release\varix-autopilot.exe up one level
  echo.
  pause
  exit /b 1
)

REM --- is the CDP port open? ---
curl -s -m 3 "http://127.0.0.1:9222/json/version" >nul 2>&1
if errorlevel 1 (
  echo.
  echo ============================================================
  echo  [WARN] Port 9222 is CLOSED - cannot read conversations
  echo ============================================================
  echo  The app cannot read your conversations until this is on.
  echo.
  echo  TO FIX - one time only:
  echo    [1] Fully quit WorkBuddy - all windows and tray icon
  echo    [2] Desktop: right-click WorkBuddy, then Properties
  echo    [3] In the Target field append one space then:
  echo         --remote-debugging-port=9222 --remote-debugging-address=127.0.0.1
  echo    [4] Relaunch WorkBuddy
  echo.
  echo  Starting anyway - the app will show exactly what to fix.
  echo ============================================================
  echo.
  timeout /t 7 /nobreak >nul
)

start "" "%AP_EXE%"
endlocal
