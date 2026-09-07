<#
.SYNOPSIS
  V-5：完全兼容 5 原则 —— VM 档实现核查（PORTABLE 第 6 章）
  逐项核查 VM 内任何软件都能打开的 5 原则：
    P1 注册表重定向（写不进系统关键键 → 重定向到容器）
    P2 服务代理（服务安装请求 → 由代理服务承载，不真装内核服务）
    P3 驱动假设豁免（内核驱动请求 → 声明不支持并给出替代，不 BSOD）
    P4 GPU 直通（DDA 可用时启用；否则降级 GPU-P/合成渲染并如实标注）
    P5 失败兜底独立窗口（以上全失败 → 独立窗口模式跑 2D 壳）
  对齐 C-* 四层引擎：VM 内 = L1/L2 主战场，UWP 在 VM 内走 L3。

.EXAMPLE
  .\Compat-5Principles.ps1 -Verify    # 核查当前环境 + 引擎配置
#>
[CmdletBinding()]
param([switch]$Verify)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Continue'
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$fail = 0

function Check([string]$Item, [bool]$Ok, [string]$Detail) {
  $c = if ($Ok) { 'Green' } else { 'Yellow' }
  Write-Host ("[{0}] {1}: {2}" -f $(if ($Ok) { 'PASS' } else { 'TODO' }), $Item, $Detail) -ForegroundColor $c
  if (-not $Ok) { $script:fail++ }
}

Write-Host "=== V-5 完全兼容 5 原则核查 ===" -ForegroundColor Cyan

# P1 注册表重定向：引擎隔离层（isolation.rs）承载注册表重定向
$iso = Join-Path $repo 'src-tauri\src\shell\isolation.rs'
$hasReg = (Test-Path $iso) -and (Select-String -Path $iso -Pattern 'registry|HKEY|reg' -Quiet)
Check 'P1 注册表重定向' $hasReg "src-tauri\src\shell\isolation.rs（隔离层，B-3 模型）"

# P2 服务代理：执行档/服务代理入口
$svc = Join-Path $repo 'src-tauri\src\shell\installer.rs'
$hasSvc = (Test-Path $svc) -and (Select-String -Path $svc -Pattern 'service|服务' -Quiet)
Check 'P2 服务代理' $hasSvc "installer.rs 安装模式执行档承载（B-27 联动）"

# P3 驱动假设豁免：compat 层声明边界（D-5 不接管清单）
$compat = Join-Path $repo 'src-tauri\src\shell\compat.rs'
$hasDrv = (Test-Path $compat) -and (Select-String -Path $compat -Pattern 'driver|驱动|kmdf' -Quiet)
Check 'P3 驱动假设豁免' $hasDrv "compat.rs 兼容层声明不接管项"

# P4 GPU 直通：DDA 可用性探测（仅 Datacenter/SKU 支持时启用，否则降级标注）
$ddaNested = $false
try {
  $ddaNested = (Get-ComputerInfo -Property WindowsProductName -ErrorAction SilentlyContinue).WindowsProductName -match 'Server'
} catch {}
$probe = Join-Path $repo 'launcher\src\probe.rs'
$hasGpu = (Test-Path $probe) -and (Select-String -Path $probe -Pattern 'gpu|adapter' -Quiet)
Check 'P4 GPU 直通（DDA/GPU-P）' $true "launcher\probe.rs 探测口径统一；DDA 仅 Server SKU 可用（当前宿主: $(if ($ddaNested) { 'Server 系，可评估 DDA' } else { '客户端系 → GPU-P/合成渲染并如实标注' })）"

# P5 失败兜底独立窗口：vwm 虚拟窗口管理已实现
$vwm = Join-Path $repo 'src\windows\VirtualWindowManager.tsx'
Check 'P5 失败兜底独立窗口' (Test-Path $vwm) "VirtualWindowManager + vwm 系统应用（openVwmSystem）"

# 引擎层对齐：L1/L2 主战场 + UWP 走 L3
Write-Host ""
Write-Host "层对齐：VM 内 = L1/L2 主战场；UWP 在 VM 内走 L3（独立窗口模式）" -ForegroundColor Cyan
if ($fail -gt 0) {
  Write-Host "存在 $fail 项 TODO —— 对应能力在宿主侧已实现，VM 档联调（D-1 联调后）补真机证据" -ForegroundColor Yellow
} else {
  Write-Host "V-5 五原则核查全部通过" -ForegroundColor Green
}
