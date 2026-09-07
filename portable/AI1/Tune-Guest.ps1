<#
.SYNOPSIS
  Variable OS · AI-1 存储核 —— 9.3/9.4/9.5 系统内一次性调优（在便携系统里跑，不动宿主）

.DESCRIPTION
  对应 docs/PORTABLE_VIRTUAL_SYSTEM_PLAN.md：
    9.3 VHDX 优化   -> compact /compactos:always、TRIM(DisableDeleteNotify=0)、Optimize-Volume -ReTrim
    9.4 系统裁剪    -> 关 SysMain/WSearch 对 VHDX 的预读与索引、关自动更新、精简 WinSxS、关休眠省 8GB
    9.5 缓存策略    -> ReadyBoost 关、建立 Data\Cache\RamCache 契约文件（256MB LRU，由 Core 消费）
    扩充 26.1-26.3  -> NtfsDisableLastAccessUpdate=1、服务启动类型、电源永不休眠

  幂等：可反复运行；每项都带「命令 + 实测校验值」，末尾打印一张验收表。

.EXAMPLE
  .\Tune-Guest.ps1
  .\Tune-Guest.ps1 -SkipCleanup            # 跳过耗时的 WinSxS ResetBase
  .\Tune-Guest.ps1 -RamDriveMB 256 -Report # 生成 RAM 缓存契约 + 报告
#>
#Requires -RunAsAdministrator
[CmdletBinding()]
param(
  [string]$DataRoot = 'D:\Data',
  [int]$RamDriveMB = 256,
  [switch]$SkipCleanup,
  [switch]$SkipTrim,
  [switch]$Report
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$rows = New-Object System.Collections.Generic.List[object]
function Add-Row {
  param([string]$Plan, [string]$Item, [string]$Cmd, [string]$Result)
  $rows.Add([pscustomobject]@{ 主计划 = $Plan; 项目 = $Item; 命令 = $Cmd; 实测 = $Result }) | Out-Null
}
function Run-Native {
  param([string]$Exe, [string[]]$Arguments)
  $out = & $Exe @Arguments 2>&1
  return ($out -join ' ').Trim()
}

Write-Host '>>> 9.3/9.4/9.5 系统内调优（在便携系统内运行，不改宿主）' -ForegroundColor Cyan

# ------------------------------------------------------------ 9.3 CompactOS
Write-Host '>>> 9.3 CompactOS' -ForegroundColor Cyan
# /compactos:always 幂等，重复执行无副作用；query 输出随系统语言变化，只作为实测值记录
Run-Native compact.exe @('/compactos:always') | Out-Null
$q2 = Run-Native compact.exe @('/compactos:query')
Add-Row '9.3' 'CompactOS 压缩系统文件（省约 30%）' 'compact /compactos:always' ($q2 -replace '\s+', ' ')

# --------------------------------------------------------------- 9.3 TRIM
Write-Host '>>> 9.3 TRIM / LastAccess' -ForegroundColor Cyan
if (-not $SkipTrim) {
  fsutil.exe behavior set DisableDeleteNotify 0 | Out-Null
}
$t = Run-Native fsutil.exe @('behavior', 'query', 'DisableDeleteNotify')
Add-Row '9.3' 'TRIM 下发（0=启用）' 'fsutil behavior set DisableDeleteNotify 0' ($t -replace '\s+', ' ')
fsutil.exe behavior set DisableLastAccess 1 | Out-Null
$la = Run-Native fsutil.exe @('behavior', 'query', 'DisableLastAccess')
Add-Row '9.2' '关闭 LastAccess 写入' 'fsutil behavior set DisableLastAccess 1' ($la -replace '\s+', ' ')

$vol = Get-Volume -DriveLetter $env:SystemDrive.TrimEnd(':') -ErrorAction SilentlyContinue
if ($vol) {
  try {
    Optimize-Volume -DriveLetter $env:SystemDrive.TrimEnd(':') -ReTrim -ErrorAction Stop | Out-Null
    Add-Row '9.3' 'VHDX 卷 ReTrim 回收' 'Optimize-Volume -ReTrim' 'OK'
  }
  catch {
    Add-Row '9.3' 'VHDX 卷 ReTrim 回收' 'Optimize-Volume -ReTrim' "SKIP（$($_.Exception.Message)）"
  }
}

# ------------------------------------------------------------ 9.4 服务裁剪
Write-Host '>>> 9.4 服务裁剪（扩充 26.2）' -ForegroundColor Cyan
$svcPlan = @(
  @{ Name = 'SysMain';   Want = 'Disabled'; Why = '关 Superfetch 对 VHDX 预读（9.4）' }
  @{ Name = 'WSearch';   Want = 'Manual';   Why = '关 Windows Search 对 VHDX 索引（9.4）' }
  @{ Name = 'DiagTrack'; Want = 'Disabled'; Why = '关诊断遥测写盘（26.2）' }
  @{ Name = 'wuauserv';  Want = 'Manual';   Why = '自动更新改手动（9.4）' }
)
foreach ($s in $svcPlan) {
  $cur = Get-Service -Name $s.Name -ErrorAction SilentlyContinue
  if (-not $cur) {
    Add-Row '9.4' $s.Why $s.Name '不存在'
    continue
  }
  Stop-Service -Name $s.Name -Force -ErrorAction SilentlyContinue
  Set-Service -Name $s.Name -StartupType $s.Want -ErrorAction SilentlyContinue
  $now = (Get-Service -Name $s.Name -ErrorAction SilentlyContinue)
  Add-Row '9.4' $s.Why "Set-Service $($s.Name) $($s.Want)" "$($now.StartType)/$($now.Status)"
}

# 关自动更新（策略级，服务被拉起也不会自动装）
$auKey = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU'
New-Item -Path $auKey -Force | Out-Null
New-ItemProperty -Path $auKey -Name NoAutoUpdate -PropertyType DWord -Value 1 -Force | Out-Null
Add-Row '9.4' '自动更新改手动' "$auKey\NoAutoUpdate=1" ((Get-ItemProperty -Path $auKey -Name NoAutoUpdate).NoAutoUpdate).ToString()

# ------------------------------------------------------------ 9.4 电源
Write-Host '>>> 9.4/26.3 电源（U 盘不休眠，省 8GB 休眠文件）' -ForegroundColor Cyan
powercfg.exe /hibernate off | Out-Null
powercfg.exe /change standby-timeout-ac 0 | Out-Null
powercfg.exe /change standby-timeout-dc 0 | Out-Null
powercfg.exe /change disk-timeout-ac 0 | Out-Null
powercfg.exe /change hibernate-timeout-ac 0 | Out-Null
$hib = Test-Path (Join-Path $env:SystemDrive 'hiberfil.sys')
Add-Row '9.4' '关休眠（释放 hiberfil.sys）' 'powercfg /hibernate off' "hiberfil.sys 存在=$hib"
Add-Row '26.3' '待机/硬盘超时置 0' 'powercfg /change *-timeout-* 0' 'OK'

# ------------------------------------------------------- 9.4 WinSxS 精简
if ($SkipCleanup) {
  Add-Row '9.4' '精简 WinSxS' 'Dism /StartComponentCleanup /ResetBase' 'SKIP（-SkipCleanup）'
}
else {
  Write-Host '>>> 9.4 精简 WinSxS（耗时数分钟）' -ForegroundColor Cyan
  $d = Run-Native dism.exe @('/Online', '/Cleanup-Image', '/StartComponentCleanup', '/ResetBase')
  Add-Row '9.4' '精简 WinSxS' 'Dism /Online /Cleanup-Image /StartComponentCleanup /ResetBase' $(if ($LASTEXITCODE -eq 0) { 'OK' } else { "exit=$LASTEXITCODE" })
}

# ------------------------------------------------------------ 9.5 缓存
Write-Host '>>> 9.5 缓存策略' -ForegroundColor Cyan
$cacheRoot = Join-Path $DataRoot 'Cache'
$ramCache = Join-Path $cacheRoot 'RamCache'
New-Item -ItemType Directory -Force -Path $ramCache | Out-Null
$contract = [pscustomobject]@{
  version    = 1
  sizeMB     = $RamDriveMB
  policy     = 'LRU'
  mountPoint = 'R:\VariableCache'
  backend    = 'ImDisk / PrimoCache 二级缓存（ReadyBoost 关闭）'
  dir        = $ramCache
  warm       = @()
}
$contractPath = Join-Path $cacheRoot 'ramcache.json'
$contract | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $contractPath -Encoding UTF8
Add-Row '9.5' "RAM 盘缓存契约（${RamDriveMB}MB LRU）" "写 $contractPath" 'OK'
Add-Row '9.5' 'ReadyBoost 关闭（改用二级缓存）' '手动：设备属性 -> ReadyBoost -> 不使用' '见文档 9.5'

# ------------------------------------------------------------ 汇总
$rows | Format-Table -AutoSize | Out-Host

if ($Report) {
  $repDir = Join-Path $DataRoot 'Cache'
  New-Item -ItemType Directory -Force -Path $repDir | Out-Null
  $rep = Join-Path $repDir ("tune-guest-{0:yyyyMMdd-HHmm}.md" -f (Get-Date))
  $md = New-Object System.Text.StringBuilder
  [void]$md.AppendLine("# 系统内调优报告 - $((Get-Date).ToString('yyyy-MM-dd HH:mm'))")
  [void]$md.AppendLine('')
  [void]$md.AppendLine('| 主计划 | 项目 | 命令 | 实测 |')
  [void]$md.AppendLine('| --- | --- | --- | --- |')
  foreach ($r in $rows) {
    [void]$md.AppendLine('| ' + $r.主计划 + ' | ' + $r.项目 + ' | `' + $r.命令 + '` | ' + $r.实测 + ' |')
  }
  $md.ToString() | Set-Content -LiteralPath $rep -Encoding UTF8
  Write-Host ">>> 报告已写入 $rep" -ForegroundColor Green
}

Write-Host '>>> 完成。下一步：.\Link-DataApps.ps1 建读写分离链接，再跑 .\Bench-Storage.ps1 出基线' -ForegroundColor Green
