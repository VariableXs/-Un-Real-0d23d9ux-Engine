@echo off
REM ============================================================
REM  VarixAutoPilot - open CDP port 9222 for WorkBuddy
REM
REM  Why this exists:
REM    The debug port is decided at process start. An already-running
REM    WorkBuddy will NOT open it no matter what you do afterwards.
REM    So the only way is: quit WorkBuddy, then start it with the flags.
REM
REM  This script does that WITHOUT touching the user's shortcut
REM  (that is the host's own file - not ours to edit) and without
REM  touching any config / user-data-dir.
REM
REM  ASKING FIRST: quitting WorkBuddy interrupts running conversations.
REM  So the default is REFUSE to kill, and prints instructions.
REM  Only when you append --kill does it actually act.
REM
REM  BUG NOTE: keep this file ASCII + CRLF. cmd reads bat as GBK and
REM  the repo path has hyphens ("-Un-Real-"), so any "set X=...\path"
REM  MUST be quoted or cmd dies with a syntax error.
REM ============================================================
setlocal

set "AP_DIR=D:\2\14\-Un-Real-0d23d9ux-Engine-main\VarixAutoPilot2"
set "PY=C:\Users\varia\.workbuddy\binaries\python\versions\3.13.12\python.exe"
set "SCRIPT=%AP_DIR%\tools\open_port.py"

if not exist "%PY%" (
  echo [ERR] python.exe not found: %PY%
  pause
  exit /b 1
)
if not exist "%SCRIPT%" (
  echo [ERR] open_port.py not found: %SCRIPT%
  pause
  exit /b 1
)

"%PY%" "%SCRIPT%" %*

echo.
pause
