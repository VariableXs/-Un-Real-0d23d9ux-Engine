@echo off
setlocal enabledelayedexpansion
title Variable Windows build

cd /d "%~dp0"
set "MODE=%~1"
if "%MODE%"=="" set "MODE=nsis"

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
cargo test
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

echo [..] running tauri %BUNDLE_ARGS% ...
call npx tauri %BUNDLE_ARGS%
if errorlevel 1 (
  echo [ERROR] tauri build failed. See messages above.
  exit /b 1
)

if /I "%MODE%"=="portable" (
  echo [..] assembling portable version...
  if exist dist-portable rmdir /s /q dist-portable
  mkdir dist-portable
  copy /y src-tauri\target\release\variable.exe dist-portable\Variable.exe >nul || (
    echo [ERROR] copy of variable.exe failed.
    exit /b 1
  )
  type nul > dist-portable\.portable
  echo [ok] portable written to dist-portable\  ^(data stored beside the exe in data\^)
)

echo ============================================================
echo  BUILD OK
echo  NSIS : src-tauri\target\release\bundle\nsis\
if /I "%MODE%"=="msi" echo  MSI  : src-tauri\target\release\bundle\msi\
echo ============================================================
exit /b 0
