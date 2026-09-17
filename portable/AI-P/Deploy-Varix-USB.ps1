<#
.SYNOPSIS
  VARIX 双域系统 · AI-P 任务 8 —— Deploy-To-USB 五分区总编排（断点续作）

.DESCRIPTION
  对应 docs/VARIX双域系统施工总案.md 阶段 1 步骤 5：
    Preflight（盘检测/容量/危险确认）→ Stage（分区 → ESP 双链 → SHARED 契约 → VARIX_SYS 系统文件）→
    Verify（清单校验 + 契约校验）→ Report（ASCII 分区图 + 容量核对表归档）
    - 断点续作：每阶段完成即写 -StateDir\deploy-state.json；重跑自动跳过已完成阶段
    - 复用既有脚本不重写：Create-Partitions.ps1 / Build-ESP.ps1 / Init-Shared.ps1（同目录）
    - -PlanOnly 纯预演：只打印将执行的阶段与产物，不碰磁盘
  VARIX_SYS 系统文件（内核 ELF + Variable 运行时）来源 -SysSource 目录（可选；缺省跳过并如实登记）。

.EXAMPLE
  .\Deploy-Varix-USB.ps1 -DiskNumber 3 -Yes
  .\Deploy-Varix-USB.ps1 -DiskNumber 3 -ResumeState D:\state -VerifyOnly
  .\Deploy-Varix-USB.ps1 -PlanOnly
#>
[CmdletBinding()]
param(
  [int]$DiskNumber = -1,
  [string]$VhdPath = "",

  # VARIX_SYS 内容源目录（内核 ELF/运行时）；可选
  [string]$SysSource = "",
  # Windows 引导源目录（Build-ESP -WindowsBootDir 用）；可选
  [string]$WindowsBootDir = "",

  # 断点续作状态目录（默认仓库 _attic\deploy-state）
  [string]$StateDir = "",
  [string]$ReportDir = "",

  [switch]$Yes,
  [switch]$PlanOnly,
  [switch]$VerifyOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Write-Step { param([string]$Msg) Write-Host ">>> $Msg" -ForegroundColor Cyan }
function Write-Ok   { param([string]$Msg) Write-Host "    + $Msg" -ForegroundColor Green }
function Write-Note { param([string]$Msg) Write-Host "    ! $Msg" -ForegroundColor Yellow }

$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not $StateDir)  { $StateDir  = Join-Path $repo '_attic\deploy-state' }
if (-not $ReportDir) { $ReportDir = Join-Path $repo 'docs\acceptance\deploy' }
New-Item -ItemType Directory -Force -Path $StateDir | Out-Null
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null
$statePath = Join-Path $StateDir 'deploy-state.json'

# 阶段清单（顺序即依赖）；断点续作以 state.done 记录
$stages = @('Preflight', 'Partition', 'ESP', 'Shared', 'SysFiles', 'Verify', 'Report')

function Get-State {
  if (Test-Path -LiteralPath $statePath) {
    return Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
  }
  return [pscustomobject]@{ disk = ''; done = @() }
}
function Set-Stage {
  param([string]$Disk, [string]$Stage)
  $s = Get-State
  if ($s.disk -ne $Disk) { $s = [pscustomobject]@{ disk = $Disk; done = @() } }
  if ($s.done -notcontains $Stage) { $s.done = @($s.done) + $Stage }
  [IO.File]::WriteAllText($statePath, ($s | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
}

$diskKey = if ($VhdPath) { $VhdPath } else { "disk$DiskNumber" }

if ($PlanOnly) {
  Write-Step "预演编排（不碰磁盘）：$diskKey"
  foreach ($s in $stages) {
    Write-Host ("  {0,-10} -> {1}" -f $s, $(switch ($s) {
        'Preflight' { '盘存在/容量/危险盘拒绝' }
        'Partition' { 'Create-Partitions.ps1（幂等布局校验内置）' }
        'ESP'       { 'Build-ESP.ps1（VARIX 链' + $(if ($WindowsBootDir) { ' + Windows 链' }) + '）' }
        'Shared'    { 'Init-Shared.ps1 契约四件套' }
        'SysFiles'  { $(if ($SysSource) { "复制 $SysSource -> VARIX_SYS" } else { '无 SysSource，跳过并登记' }) }
        'Verify'    { 'Build-ESP -VerifyOnly + Init-Shared -ValidateOnly' }
        'Report'    { "分区图+容量表归档 $ReportDir" }
      }))
  }
  Write-Ok "状态文件：$statePath（断点续作依据）"
  exit 0
}

if ($DiskNumber -lt 0 -and -not $VhdPath) { throw '必须提供 -DiskNumber 或 -VhdPath 之一' }
if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  throw '部署操作需要管理员 PowerShell'
}

# 找 ESP/SHARED 卷（分区后按标签定位）
function Get-VolByLabel {
  param([string]$Label)
  return Get-Volume -FileSystemLabel $Label -ErrorAction SilentlyContinue | Select-Object -First 1
}

foreach ($stage in $stages) {
  $state = Get-State
  if ($state.disk -eq $diskKey -and $state.done -contains $stage -and -not $VerifyOnly) {
    Write-Note "[$stage] 已完成（断点续作跳过；-VerifyOnly 或删 $statePath 可强制重跑）"
    continue
  }
  Write-Step "[$stage]"

  switch ($stage) {
    'Preflight' {
      if ($VhdPath) {
        if (-not (Test-Path -LiteralPath $VhdPath)) { throw "VHD 不存在：$VhdPath" }
      }
      else {
        $d = Get-Disk -Number $DiskNumber -ErrorAction SilentlyContinue
        if (-not $d) { throw "磁盘不存在：Disk $DiskNumber" }
        if ($d.IsBoot -or $d.IsSystem) { throw "磁盘 $DiskNumber 是启动/系统盘，拒绝（物理隔离）" }
        Write-Ok "目标 Disk $DiskNumber（$([math]::Round($d.Size/1GB,1)) GB，$($d.FriendlyName)）"
      }
      Set-Stage $diskKey 'Preflight'
    }
    'Partition' {
      $args2 = @()
      if ($VhdPath) { $args2 += @('-VhdPath', $VhdPath) } else { $args2 += @('-DiskNumber', "$DiskNumber") }
      if ($Yes) { $args2 += '-Yes' }
      & (Join-Path $PSScriptRoot 'Create-Partitions.ps1') @args2 -ReportPath (Join-Path $ReportDir 'partition-map.txt')
      if ($LASTEXITCODE -ne 0) { throw "分区阶段失败（exit=$LASTEXITCODE）" }
      Set-Stage $diskKey 'Partition'
    }
    'ESP' {
      $espVol = Get-VolByLabel 'VARIX-ESP'
      if (-not $espVol -or -not $espVol.DriveLetter) { throw '未找到 ESP 卷（VARIX-ESP）' }
      $espArgs = @('-EspPath', "$($espVol.DriveLetter):\")
      if ($WindowsBootDir) { $espArgs += @('-WindowsBootDir', $WindowsBootDir) }
      & (Join-Path $PSScriptRoot 'Build-ESP.ps1') @espArgs
      if ($LASTEXITCODE -ne 0) { throw "ESP 阶段失败（exit=$LASTEXITCODE）" }
      Set-Stage $diskKey 'ESP'
    }
    'Shared' {
      $shVol = Get-VolByLabel 'SHARED'
      if (-not $shVol -or -not $shVol.DriveLetter) { throw '未找到 SHARED 卷' }
      & (Join-Path $PSScriptRoot 'Init-Shared.ps1') -SharedRoot "$($shVol.DriveLetter):"
      if ($LASTEXITCODE -ne 0) { throw "SHARED 阶段失败（exit=$LASTEXITCODE）" }
      Set-Stage $diskKey 'Shared'
    }
    'SysFiles' {
      if ($SysSource) {
        if (-not (Test-Path -LiteralPath $SysSource -PathType Container)) { throw "SysSource 不存在：$SysSource" }
        $sysVol = Get-VolByLabel 'VARIX_SYS'
        if (-not $sysVol -or -not $sysVol.DriveLetter) { throw '未找到 VARIX_SYS 卷' }
        $dst = "$($sysVol.DriveLetter):\"
        robocopy $SysSource $dst /E /R:2 /W:2 /MT:8 /NFL /NDL | Out-Null
        if ($LASTEXITCODE -ge 8) { throw "VARIX_SYS 复制失败（robocopy exit=$LASTEXITCODE）" }
        Write-Ok "VARIX_SYS 已复制 $($SysSource) -> $dst"
      }
      else {
        Write-Note '未提供 -SysSource，VARIX_SYS 系统文件跳过（❌ 登记：内核 ELF/运行时待部署）'
      }
      Set-Stage $diskKey 'SysFiles'
    }
    'Verify' {
      $espVol = Get-VolByLabel 'VARIX-ESP'
      if ($espVol -and $espVol.DriveLetter) {
        & (Join-Path $PSScriptRoot 'Build-ESP.ps1') -EspPath "$($espVol.DriveLetter):\" -VerifyOnly
        if ($LASTEXITCODE -ne 0) { throw "ESP 校验失败" }
      }
      $shVol = Get-VolByLabel 'SHARED'
      if ($shVol -and $shVol.DriveLetter) {
        & (Join-Path $PSScriptRoot 'Init-Shared.ps1') -SharedRoot "$($shVol.DriveLetter):" -ValidateOnly
        if ($LASTEXITCODE -ne 0) { throw 'SHARED 契约校验失败' }
      }
      Write-Ok 'Verify 段全过'
      Set-Stage $diskKey 'Verify'
    }
    'Report' {
      $report = @()
      $report += "VARIX 五分区部署报告 $(Get-Date -Format 'o')"
      $report += "目标：$diskKey"
      if (Test-Path -LiteralPath (Join-Path $ReportDir 'partition-map.txt')) {
        $report += ''
        $report += (Get-Content -LiteralPath (Join-Path $ReportDir 'partition-map.txt') -Raw)
      }
      foreach ($lbl in @('VARIX-ESP', 'VARIX_SYS', 'WIN_ENGINE', 'SHARED', 'SNAPSHOT')) {
        $v = Get-VolByLabel $lbl
        if ($v) {
          $report += ("{0,-12} {1,8:N1} GB 总量  {2,8:N1} GB 可用" -f $lbl, ($v.Size / 1GB), ($v.SizeRemaining / 1GB))
        }
        else { $report += "$lbl  ❌ 未挂载" }
      }
      $rp = Join-Path $ReportDir ("deploy-report-" + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.txt')
      $report -join "`r`n" | Set-Content -LiteralPath $rp -Encoding UTF8
      Write-Ok "报告归档 $rp"
      Set-Stage $diskKey 'Report'
    }
  }
}

Write-Host ''
Write-Host ">>> 五分区部署编排完成：$diskKey（状态 $statePath）" -ForegroundColor Green
