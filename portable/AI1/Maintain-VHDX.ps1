<#
.SYNOPSIS
  Variable OS · AI-1 存储核 —— 3.3 / 9.3 每月维护：Optimize-VHD + ReTrim + 碎片率体检

.DESCRIPTION
  对应 docs/PORTABLE_VIRTUAL_SYSTEM_PLAN.md：
    3.3 动态 vs 固定 -> 定期 Optimize-VHD -Mode Full 回收未用块（每月一次）
    3.1 验收         -> Get-VHD 碎片率 < 5%
    9.3 VHDX 优化    -> 宿主卷 Optimize-Volume -ReTrim 把空闲块 TRIM 下发给固态盘
    3.1 差分链       -> -Merge 把 User.vhdx 合并回 Apps 层（合并前自动备份提示）

  注意：Optimize-VHD 要求 VHDX 未挂载（虚拟机必须关机）。

.EXAMPLE
  .\Maintain-VHDX.ps1 -Vhdx D:\Variable-USB\Variable-OS.vhdx
  .\Maintain-VHDX.ps1 -Vhdx D:\Variable-USB\User.vhdx -Merge -OutFile D:\Variable-USB\Data\Cache\maintain.md
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)]
  [string]$Vhdx,

  [ValidateSet('Full', 'Quick', 'Retrim', 'Pretrimmed', 'Prezeroed')]
  [string]$Mode = 'Full',

  # 差分链：把该差分子盘合并回其父盘（3.1）
  [switch]$Merge,

  [int]$FragThreshold = 5,

  [switch]$SkipHostTrim,

  [string]$OutFile
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not (Get-Module -ListAvailable -Name Hyper-V)) { throw '未检测到 Hyper-V 模块（Optimize-VHD 依赖它）' }
Import-Module Hyper-V
if (-not (Test-Path -LiteralPath $Vhdx)) { throw "VHDX 不存在：$Vhdx" }

$before = Get-VHD -Path $Vhdx
Write-Host (">>> 维护 {0}（{1} / 虚拟 {2}GB / 实占 {3}GB / 碎片 {4}%）" -f `
    $Vhdx, $before.VhdType, [math]::Round($before.Size / 1GB, 1), `
    [math]::Round($before.FileSize / 1GB, 2), $before.FragmentationPercentage) -ForegroundColor Cyan

if ($before.Attached) {
  throw 'VHDX 处于挂载状态，无法 Optimize：请先关闭便携虚拟机（或 Dismount-VHD）'
}

$t0 = [Diagnostics.Stopwatch]::StartNew()
Optimize-VHD -Path $Vhdx -Mode $Mode
$t0.Stop()
Write-Host ("    + Optimize-VHD -Mode {0} 完成（{1}s）" -f $Mode, [math]::Round($t0.Elapsed.TotalSeconds, 1)) -ForegroundColor Green

if ($Merge) {
  $parent = $before.ParentPath
  if (-not $parent) {
    Write-Host '    ! 该盘不是差分盘，无父盘可合并，跳过 -Merge' -ForegroundColor Yellow
  }
  else {
    Write-Host "    ! 即将把 $Vhdx 合并回父盘 $parent（不可逆）" -ForegroundColor Yellow
    Merge-VHD -Path $Vhdx -DestinationPath $parent
    Write-Host "    + 已合并；差分盘文件已被吸收，建议随后 Optimize-VHD -Path `"$parent`" -Mode Full" -ForegroundColor Green
    $Vhdx = $parent
  }
}

$after = Get-VHD -Path $Vhdx
$savedGB = [math]::Round(($before.FileSize - $after.FileSize) / 1GB, 2)
$fragOK = $after.FragmentationPercentage -le $FragThreshold

# 9.3：把 VHDX 所在宿主卷的空闲块 ReTrim 下发，固态盘才能真正回收
$hostTrim = 'SKIP'
if (-not $SkipHostTrim) {
  try {
    $root = [IO.Path]::GetPathRoot((Resolve-Path -LiteralPath (Split-Path -Parent $Vhdx)).Path)
    $letter = $root.TrimEnd('\', ':')
    Optimize-Volume -DriveLetter $letter -ReTrim -ErrorAction Stop | Out-Null
    $hostTrim = "OK($letter)"
  }
  catch {
    $hostTrim = "FAIL（$($_.Exception.Message)）"
  }
}

$rows = @(
  [pscustomobject]@{ 项 = '类型'; 前 = $before.VhdType; 后 = $after.VhdType }
  [pscustomobject]@{ 项 = '实占GB'; 前 = [math]::Round($before.FileSize / 1GB, 2); 后 = [math]::Round($after.FileSize / 1GB, 2) }
  [pscustomobject]@{ 项 = '碎片率pct'; 前 = $before.FragmentationPercentage; 后 = $after.FragmentationPercentage }
  [pscustomobject]@{ 项 = '本次回收GB'; 前 = ''; 后 = $savedGB }
  [pscustomobject]@{ 项 = '宿主卷 ReTrim'; 前 = ''; 后 = $hostTrim }
  [pscustomobject]@{ 项 = "碎片判定(< $FragThreshold%)"; 前 = ''; 后 = $(if ($fragOK) { 'PASS' } else { 'FAIL' }) }
)
$rows | Format-Table -AutoSize | Out-Host

if ($OutFile) {
  $md = New-Object System.Text.StringBuilder
  [void]$md.AppendLine("# VHDX 维护记录 - $((Get-Date).ToString('yyyy-MM-dd HH:mm'))")
  [void]$md.AppendLine('')
  [void]$md.AppendLine("目标：``$Vhdx``  模式：$Mode")
  [void]$md.AppendLine('')
  [void]$md.AppendLine('| 项 | 前 | 后 |')
  [void]$md.AppendLine('| --- | --- | --- |')
  foreach ($r in $rows) {
    [void]$md.AppendLine('| ' + $r.项 + ' | ' + $r.前 + ' | ' + $r.后 + ' |')
  }
  $md.ToString() | Set-Content -LiteralPath $OutFile -Encoding UTF8
  Write-Host ">>> 记录已写入 $OutFile" -ForegroundColor Green
}

Write-Host '>>> 建议节奏：Optimize-VHD 每周/每月一次，defrag <卷> /O 每月一次（主计划 26.4）' -ForegroundColor Green
