<#
.SYNOPSIS
  V-6：层式镜像 —— 基础层 + 应用层 + 用户层三差分链（PORTABLE 第 8 章）
  在 V-2 Diff-Chain（系统盘单链）之上扩展为三层：
    base 层：Variable-OS-base.vhdx（只读，系统底座）
    app  层：Variable-OS-app.vhdx（差分于 base；MSIX/软件安装落此层，可独立重置）
    user 层：Variable-OS-user.vhdx（差分于 app；每会话差分，重置即丢，数据盘不受影响）
  插件化：X-* 扩展协议在 VM 档走同一套桥接（vm_agent 心跳/回调不变）。

.EXAMPLE
  .\Layer-Chain.ps1 -Action New    -Base D:\USB\Variable-OS-base.vhdx
  .\Layer-Chain.ps1 -Action Health -Base D:\USB\Variable-OS-base.vhdx
  .\Layer-Chain.ps1 -Action Reset  -Base D:\USB\Variable-OS-base.vhdx -Layer user   # 只重置用户层
  .\Layer-Chain.ps1 -Action Merge  -Base D:\USB\Variable-OS-base.vhdx -Layer app    # app 层合并回 base（停机维护）
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)]
  [ValidateSet('New', 'Health', 'Reset', 'Merge')]
  [string]$Action,
  [Parameter(Mandatory = $true)]
  [string]$Base,
  [ValidateSet('app', 'user')]
  [string]$Layer = 'user',
  [switch]$WhatIfChainOnly
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not (Get-Module -ListAvailable -Name Hyper-V)) { throw '未检测到 Hyper-V 模块（New-VHD 依赖）' }
Import-Module Hyper-V
if (-not (Test-Path -LiteralPath $Base)) { throw "base 层不存在：$Base" }
$dir = Split-Path -Parent $Base
$appVhdx = Join-Path $dir 'Variable-OS-app.vhdx'
$userVhdx = Join-Path $dir 'Variable-OS-user.vhdx'

function Assert-Detached([string]$Path) {
  if ((Get-VHD -Path $Path -ErrorAction SilentlyContinue).Attached) { throw "磁盘挂载中，请先关机卸盘：$Path" }
}

switch ($Action) {
  'New' {
    if (-not $WhatIfChainOnly) { Set-ItemProperty -Path $Base -Name IsReadOnly -Value $true }
    foreach ($pair in @(@('app', $Base, $appVhdx), @('user', $appVhdx, $userVhdx))) {
      $name = $pair[0]; $parent = $pair[1]; $child = $pair[2]
      if (Test-Path -LiteralPath $child) { Write-Host "    $name 层已存在：$child（跳过）" -ForegroundColor Yellow; continue }
      if (-not (Test-Path -LiteralPath $parent)) { throw "父层缺失：$parent（先建 $name 层的父层）" }
      New-VHD -Path $child -ParentPath $parent -Differencing | Out-Null
      Write-Host "    $name 层已建立：$child（父：$(Split-Path -Leaf $parent)）" -ForegroundColor Green
    }
    Write-Host ">>> 三层链就绪：base(只读) <- app <- user。VM 挂 user 层启动；MSIX-Attach 写 app 层（停机挂载 app 层安装）" -ForegroundColor Green
  }
  'Health' {
    foreach ($v in @($Base, $appVhdx, $userVhdx)) {
      if (-not (Test-Path -LiteralPath $v)) { Write-Host "FAIL 层缺失：$v" -ForegroundColor Red; continue }
      $h = Get-VHD -Path $v
      $parentOk = -not $h.ParentPath -or (Test-Path -LiteralPath $h.ParentPath)
      Write-Host ("{0} {1}  type={2} parent={3}" -f $(if ($parentOk) { 'PASS' } else { 'FAIL' }), (Split-Path -Leaf $v), $h.VhdType, $(Split-Path -Leaf $h.ParentPath)) -ForegroundColor $(if ($parentOk) { 'Green' } else { 'Red' })
    }
    $baseRO = (Get-Item -LiteralPath $Base).IsReadOnly
    Write-Host ("{0} base 层只读状态: {1}" -f $(if ($baseRO) { 'PASS' } else { 'WARN' }), $baseRO) -ForegroundColor $(if ($baseRO) { 'Green' } else { 'Yellow' })
  }
  'Reset' {
    $target = if ($Layer -eq 'app') { $appVhdx } else { $userVhdx }
    Assert-Detached $target
    Remove-Item -LiteralPath $target -Force
    $newParent = if ($Layer -eq 'app') { $Base } else { $appVhdx }
    New-VHD -Path $target -ParentPath $newParent -Differencing | Out-Null
    Write-Host ">>> $Layer 层已重置（父层与数据盘不受影响）" -ForegroundColor Green
    Write-Host "    注意：重置 app 层会使 user 层父引用失效 —— 请随后重置 user 层" -ForegroundColor Yellow
    if ($Layer -eq 'app') {
      Remove-Item -LiteralPath $userVhdx -Force -ErrorAction SilentlyContinue
      New-VHD -Path $userVhdx -ParentPath $appVhdx -Differencing | Out-Null
      Write-Host "    user 层已同步重建" -ForegroundColor Green
    }
  }
  'Merge' {
    # 停机维护：把 app 层合并进 base（base 转可写 → Merge → 转只读），user 层重建
    Assert-Detached $Base; Assert-Detached $appVhdx; Assert-Detached $userVhdx
    Set-ItemProperty -Path $Base -Name IsReadOnly -Value $false
    try { Merge-VHD -Path $appVhdx -DestinationPath $Base | Out-Null }
    finally { Set-ItemProperty -Path $Base -Name IsReadOnly -Value $true }
    Remove-Item -LiteralPath $appVhdx -Force; Remove-Item -LiteralPath $userVhdx -Force
    New-VHD -Path $appVhdx -ParentPath $Base -Differencing | Out-Null
    New-VHD -Path $userVhdx -ParentPath $appVhdx -Differencing | Out-Null
    Write-Host ">>> app 层已合并回 base，链已重建" -ForegroundColor Green
  }
}
