@echo off
setlocal enabledelayedexpansion
title Variable Windows build

REM ---------------------------------------------------------------------------
REM  This file MUST keep CRLF line endings and stay ASCII-only.
REM  cmd.exe mis-parses LF-only batch files (breaks "if ( )" / "for" blocks),
REM  and non-ASCII comments desync its parser.
REM  .gitattributes pins "*.bat text eol=crlf" so a fresh clone stays correct.
REM ---------------------------------------------------------------------------

cd /d "%~dp0"
set "MODE=%~1"
if "%MODE%"=="" set "MODE=nsis"

if /I not "%MODE%"=="nsis" if /I not "%MODE%"=="msi" if /I not "%MODE%"=="portable" if /I not "%MODE%"=="demo" (
  echo [ERROR] Unknown mode "%MODE%".
  echo         Usage: build-windows.bat nsis / msi / portable / demo
  exit /b 2
)

echo ============================================================
echo  Variable build - mode: %MODE%
echo ============================================================

where node >nul 2>nul
if errorlevel 1 (
  echo [ERROR] Node.js not found in PATH. Install Node 20+ from https://nodejs.org
  exit /b 1
)
for /f "delims=" %%v in ('node --version') do echo [ok] node %%v

where npm >nul 2>nul
if errorlevel 1 (
  echo [ERROR] npm not found in PATH.
  exit /b 1
)

where rustc >nul 2>nul
if errorlevel 1 (
  echo [ERROR] rustc not found. Install via https://rustup.rs - toolchain stable-msvc required.
  exit /b 1
)
for /f "delims=" %%v in ('rustc --version') do echo [ok] %%v

where cargo >nul 2>nul
if errorlevel 1 (
  echo [ERROR] cargo not found.
  exit /b 1
)
for /f "delims=" %%v in ('cargo --version') do echo [ok] %%v

if not exist node_modules (
  echo [..] installing npm dependencies...
  call npm install --no-audit --no-fund
  if errorlevel 1 (
    echo [ERROR] npm install failed.
    exit /b 1
  )
)

if not exist "node_modules\.bin\tauri.cmd" (
  echo [ERROR] tauri CLI missing at node_modules\.bin\tauri.cmd
  echo         Run: npm install
  exit /b 1
)

echo [..] typecheck...
call npx tsc --noEmit
if errorlevel 1 (
  echo [ERROR] TypeScript check failed.
  exit /b 1
)

echo [..] frontend unit tests...
call npx vitest run
if errorlevel 1 (
  echo [ERROR] frontend tests failed.
  exit /b 1
)

echo [..] backend tests...
pushd src-tauri
cargo test --workspace
if errorlevel 1 (
  popd
  echo [ERROR] Rust tests failed.
  exit /b 1
)
popd

if not exist src-tauri\icons\icon.ico (
  echo [..] generating icons...
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\gen-icons.ps1
  if errorlevel 1 (
    echo [ERROR] icon generation failed.
    exit /b 1
  )
)

set "BUNDLE_ARGS=build --bundles nsis"
if /I "%MODE%"=="msi" set "BUNDLE_ARGS=build --bundles nsis,msi"
if /I "%MODE%"=="demo" set "BUNDLE_ARGS=build --bundles none"

REM ---- real-machine fix: stop repo-local instances before bundling ----
REM If target\release\variable.exe is held by a running instance, cargo fails
REM to replace the exe at link time with "Access denied (os error 5)".
REM The helper only closes instances whose exe lives inside this repo
REM (target\release, dist-portable); it never touches installs elsewhere.
echo [..] stopping Variable instances from this repo ...
powershell -NoProfile -ExecutionPolicy Bypass -File tools\stop-variable.ps1
if errorlevel 1 (
  echo [ERROR] variable.exe is still locked by a running process. Close Variable and retry.
  exit /b 1
)

echo [..] running tauri %BUNDLE_ARGS% ...
call npx tauri %BUNDLE_ARGS%
if errorlevel 1 (
  echo [ERROR] tauri build failed. See messages above.
  exit /b 1
)

if not exist src-tauri\target\release\variable.exe (
  echo [ERROR] src-tauri\target\release\variable.exe was not produced.
  exit /b 1
)

if /I "%MODE%"=="portable" (
  echo [..] assembling portable version...
  if exist dist-portable rmdir /s /q dist-portable
  if exist dist-portable (
    echo [ERROR] could not clear dist-portable - a file there may be in use.
    exit /b 1
  )
  mkdir dist-portable
  copy /y src-tauri\target\release\variable.exe dist-portable\Variable.exe >nul
  if errorlevel 1 (
    echo [ERROR] copy of variable.exe failed.
    exit /b 1
  )
  type nul > dist-portable\.portable
  echo [ok] portable written to dist-portable\ - data stored beside the exe in data\
)

echo ============================================================
echo  BUILD OK
echo ============================================================
set "BUNDLE_ROOT=%~dp0src-tauri\target\release\bundle"
set "ARTIFACT_FOUND="
if /I not "%MODE%"=="demo" (
  for %%I in ("%BUNDLE_ROOT%\nsis\*.exe") do if exist "%%~fI" (
    echo   NSIS     : %%~fI
    set "ARTIFACT_FOUND=1"
  )
)
if /I "%MODE%"=="msi" (
  for %%I in ("%BUNDLE_ROOT%\msi\*.msi") do if exist "%%~fI" echo   MSI      : %%~fI
)
if /I "%MODE%"=="portable" (
  for %%I in ("%~dp0dist-portable\Variable.exe") do if exist "%%~fI" (
    echo   PORTABLE : %%~fI
    set "ARTIFACT_FOUND=1"
  )
)
if /I "%MODE%"=="demo" set "ARTIFACT_FOUND=1"

if not defined ARTIFACT_FOUND (
  echo [ERROR] build reported success but no bundle artifact was produced.
  exit /b 1
)
echo ============================================================
exit /b 0
