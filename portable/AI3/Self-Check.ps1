# V-1：AI-3 自检（十步收尾的核查部分）
# - 本目录脚本语法校验
# - launcher/ 契约常量与本文档对齐（47631/47632/VAR_RUNTIME_MODE）
# - launcher 构建产物存在性（可选）
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$root = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent  # repo root
$fail = 0

# 1) 语法校验
foreach ($f in Get-ChildItem $PSScriptRoot -Filter *.ps1) {
  $errs = $null
  [System.Management.Automation.Language.Parser]::ParseFile($f.FullName, [ref]$null, [ref]$errs) | Out-Null
  if ($errs.Count -gt 0) { Write-Host "FAIL 语法 $($f.Name): $($errs[0])" -ForegroundColor Red; $fail++ }
  else { Write-Host "OK    语法 $($f.Name)" -ForegroundColor Green }
}

# 2) 契约常量核对（launcher / 引擎源码 vs 本 README）
foreach ($pair in @(
  @{ p = "src-tauri\src\vm_agent.rs"; kw = "47631" },
  @{ p = "src-tauri\src\vm_agent.rs"; kw = "47632" },
  @{ p = "src-tauri\src\shell\sysinfo.rs"; kw = "VAR_RUNTIME_MODE" },
  @{ p = "launcher\src\degrade.rs"; kw = "VAR_RUNTIME_MODE" },
  @{ p = "launcher\src\probe.rs"; kw = "probe-cache.json" }
)) {
  $p = Join-Path $root $pair.p
  if ((Test-Path $p) -and (Select-String -Path $p -Pattern $pair.kw -Quiet)) {
    Write-Host "OK    契约 $($pair.p) 含 $($pair.kw)" -ForegroundColor Green
  } else {
    Write-Host "FAIL 契约 $($pair.p) 缺 $($pair.kw)" -ForegroundColor Red; $fail++
  }
}

# 3) 构建产物（可选，仅提示）
$exe = Get-ChildItem (Join-Path $root "launcher\target\release\Variable-Launcher.exe") -ErrorAction SilentlyContinue
if ($exe) { Write-Host ("OK    引导器产物 {0:N1} MB" -f ($exe.Length/1MB)) -ForegroundColor Green }
else { Write-Host "WARN 引导器未构建（cd launcher; cargo build --release）" -ForegroundColor Yellow }

if ($fail -gt 0) { exit 1 } else { Write-Host "AI-3 自检全部通过" -ForegroundColor Cyan }
