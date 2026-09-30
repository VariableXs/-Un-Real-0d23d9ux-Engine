# M-83 键位 CI 门禁 — pre-commit 钩子安装器（手动安装口径，不引 husky 依赖）。
# 用法：powershell -ExecutionPolicy Bypass -File tools/hooks/install.ps1
$root = (Split-Path -Parent $PSScriptRoot | Split-Path -Parent)
$src = Join-Path $PSScriptRoot "pre-commit"
$hooksDir = Join-Path $root ".git\hooks"
$dst = Join-Path $hooksDir "pre-commit"

if (-not (Test-Path $hooksDir)) { Write-Error "not a git worktree: $hooksDir"; exit 1 }
Copy-Item $src $dst -Force
Write-Host "M-83 pre-commit hook installed -> $dst"
Write-Host "uninstall: Remove-Item '$dst'"
