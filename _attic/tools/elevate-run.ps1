# elevate-run.ps1 — 提权运行一个本仓库的 .ps1 体检/防护脚本（2026-09-22）。
#
# 用法（普通 PowerShell 窗口即可）：
#   powershell -File _attic\tools\elevate-run.ps1 -Script _attic\usb-bcd-guard.ps1
#   powershell -File _attic\tools\elevate-run.ps1 -Script _attic\tools\nvram-watch-readonly.ps1
#
# 行为：非管理员 → 自举 UAC（Start-Process -Verb RunAs 重启自身）→ 管理员侧
# 把目标脚本的 {log} 占位符替换成 %TEMP% 实路径（临时副本，原文件不动）→
# 执行 → 报告既在 UAC 窗口回显，也落 %TEMP%\vx-*.log 与 _attic\reports\。
param(
  [Parameter(Mandatory = $true)][string]$Script
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$src = Join-Path (Get-Location) $Script
if (-not [IO.File]::Exists($src)) { throw "script not found: $src" }
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$name = [IO.Path]::GetFileNameWithoutExtension($Script)
$reportDir = Join-Path (Split-Path (Split-Path $src)) 'reports'
if ($name -eq 'nvram-watch-readonly') { $reportDir = Join-Path (Split-Path $src) 'reports' }
New-Item -ItemType Directory -Force -Path $reportDir | Out-Null
$log = Join-Path $reportDir ($name + '-' + $stamp + '.log')

$id = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $id.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  Write-Output ("[elevate] relaunching elevated: " + $Script)
  $tmp = Join-Path $env:TEMP ('vx-elevate-' + $name + '.ps1')
  $body = [IO.File]::ReadAllText($src)
  [IO.File]::WriteAllText($tmp, ($body -replace '\{log\}', $log.Replace('\', '\\')), (New-Object System.Text.UTF8Encoding($true)))
  $p = Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -PassThru -ArgumentList @(
    '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $tmp
  )
  Write-Output ("[elevate] elevated exit code: " + $p.ExitCode)
  if (Test-Path $log) {
    Write-Output ("[elevate] report: " + $log)
    Get-Content $log | ForEach-Object { Write-Output ("  | " + $_) }
    Copy-Item $log (Join-Path $reportDir ($name + '-' + $stamp + '.log')) -Force
  } else {
    Write-Output '[elevate] no report produced (window closed early?) - temp script at ' + $tmp
  }
  exit $p.ExitCode
}

# 已是管理员：直接以管理员身份跑（占位符同样替换，临时副本执行）。
$tmp = Join-Path $env:TEMP ('vx-elevate-' + $name + '.ps1')
$body = [IO.File]::ReadAllText($src)
[IO.File]::WriteAllText($tmp, ($body -replace '\{log\}', $log.Replace('\', '\\')), (New-Object System.Text.UTF8Encoding($true)))
& $tmp
$rc = $LASTEXITCODE
if (Test-Path $log) {
  Copy-Item $log (Join-Path $reportDir ($name + '-' + $stamp + '.log')) -Force
  Write-Output ("[elevate] report: " + (Join-Path $reportDir ($name + '-' + $stamp + '.log')))
}
exit $rc
