<#
.SYNOPSIS
  V-2：VHDX 差分链 + 读写分离（AI-3 引导路）
  基础盘只读（Variable-OS-base.vhdx）+ 差分盘链（每会话/每回滚点）；
  系统盘差分可随时重置回基础盘（一键还原）；数据盘独立（Uxv 容器），永不重置。
  Maintain-VHDX.ps1（AI-1）负责 Optimize/Merge；本脚本负责链的建立/体检/重置。

.EXAMPLE
  .\Diff-Chain.ps1 -Action New        -Base D:\USB\Variable-OS.vhdx
  .\Diff-Chain.ps1 -Action Health     -Base D:\USB\Variable-OS.vhdx
  .\Diff-Chain.ps1 -Action Reset      -Base D:\USB\Variable-OS.vhdx   # 糟蹋系统盘后 60s 还原
  .\Diff-Chain.ps1 -Action Checkpoint -Base D:\USB\Variable-OS.vhdx   # 建快照回滚点
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)]
  [ValidateSet('New', 'Health', 'Reset', 'Checkpoint')]
  [string]$Action,

  [Parameter(Mandatory = $true)]
  [string]$Base,

  # 差分链层名（New/Checkpoint 用）：daily-YYYYMMDD / manual-*
  [string]$Layer = "",

  # 数据盘（读写分离：独立 VHDX，绝不参与重置）
  [string]$DataVhdx = "",

  [switch]$WhatIfChainOnly
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not (Get-Module -ListAvailable -Name Hyper-V)) { throw '未检测到 Hyper-V 模块（New-VHD 依赖）' }
Import-Module Hyper-V
if (-not (Test-Path -LiteralPath $Base)) { throw "基础盘不存在：$Base" }
$dir = Split-Path -Parent $Base
if (-not $Layer) { $Layer = 'daily-' + (Get-Date -Format 'yyyyMMdd') }
$diff = Join-Path $dir ("Variable-OS-diff-{0}.vhdx" -f $Layer)
$latest = Join-Path $dir 'Variable-OS-diff-latest.txt'

function Get-Chain([string]$leaf) {
  # 自叶到基逐级取父，输出链（健康检查用）
  $chain = @(); $cur = $leaf
  while ($cur) {
    $v = Get-VHD -Path $cur -ErrorAction Stop
    $chain += [pscustomobject]@{ Path = $cur; Type = $v.VhdType; Attached = $v.Attached; Parent = $v.ParentPath }
    $cur = $v.ParentPath
  }
  return $chain
}

switch ($Action) {
  'New' {
    # 基础盘转只读（防误写，读写分离第一道）
    if (-not $WhatIfChainOnly) { Set-ItemProperty -Path $Base -Name IsReadOnly -Value $true }
    if (Test-Path -LiteralPath $diff) { throw "差分盘已存在：$diff（沿用或先 Reset）" }
    New-VHD -Path $diff -ParentPath $Base -Differencing | Out-Null
    $diff | Set-Content -LiteralPath $latest -Encoding UTF8
    Write-Host ">>> 差分盘已建立：$diff（父盘 $Base 已置只读）" -ForegroundColor Green
    Write-Host "    VM 用差分盘启动：Set-VM ... -VHDPath `"$diff`"" -ForegroundColor Cyan
    Write-Host "    数据盘 $DataVhdx 独立挂载，重置不受影响（读写分离）" -ForegroundColor Cyan
  }
  'Checkpoint' {
    if (-not (Test-Path -LiteralPath $diff)) { throw "当前差分盘不存在：先 New" }
    $cp = Join-Path $dir ("Variable-OS-diff-{0}.vhdx" -f ('cp-' + (Get-Date -Format 'yyyyMMdd-HHmmss')))
    New-VHD -Path $cp -ParentPath $diff -Differencing | Out-Null
    $cp | Set-Content -LiteralPath $latest -Encoding UTF8
    Write-Host ">>> 回滚点已建立：$cp" -ForegroundColor Green
  }
  'Health' {
    $leaf = if (Test-Path -LiteralPath $latest) { (Get-Content -LiteralPath $latest -Raw).Trim() } else { $Base }
    $chain = Get-Chain $leaf
    $chain | Format-Table Path, Type, Attached -AutoSize | Out-Host
    $broken = $chain | Where-Object { $_.Parent -and -not (Test-Path -LiteralPath $_.Parent) }
    $writableBase = ($chain | Select-Object -Last 1) -and -not (Get-Item -LiteralPath $Base).IsReadOnly
    if ($broken) { Write-Host "FAIL 断链：$($broken.Path)" -ForegroundColor Red; exit 1 }
    if ($writableBase) { Write-Host "WARN 基础盘未置只读（读写分离不完整）" -ForegroundColor Yellow }
    else { Write-Host "PASS 差分链完整，基础盘只读" -ForegroundColor Green }
  }
  'Reset' {
    if ((Get-VHD -Path $Base).Attached) { throw '基础盘挂载中，请先关机卸盘' }
    # 只删差分层，基础盘/数据盘不动 —— 60s 回基础态
    foreach ($d in (Get-ChildItem $dir -Filter 'Variable-OS-diff-*.vhdx')) {
      if ($d.FullName -ne $Base) {
        try { Dismount-VHD -Path $d.FullName -ErrorAction Stop } catch {}
        Remove-Item -LiteralPath $d.FullName -Force
        Write-Host "    - 已移除差分层 $($d.Name)" -ForegroundColor Yellow
      }
    }
    if ($DataVhdx -and (Test-Path -LiteralPath $DataVhdx)) {
      Write-Host "    = 数据盘 $DataVhdx 原样保留（永不重置）" -ForegroundColor Green
    }
    Write-Host ">>> 重置完成。重新执行 -Action New 即可回到基础态（数据盘独立不受影响）" -ForegroundColor Green
  }
}
