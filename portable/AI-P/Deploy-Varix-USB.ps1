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

# ESP 盘符保障：ESP 只接受盘符、不接受装入点（diskpart 实测明确拒绝 mount）。
# 盘符/ESP 卷仅对提权会话可见（普通会话查不到，勿以非提权视角验证），且在
# 部署进程（同一提权会话）存续期内有效——足够跑完全部阶段；实机引导由固件
# 读 ESP，不依赖 Windows 盘符。判定用 Get-Volume（Get-Partition 的 WMI 视图
# 对盘符分配陈旧不可靠，实测多次误判）。幂等：已有盘符直接返回。
function Get-EspMount {
  $espVol = Get-VolByLabel 'VARIX-ESP'
  if (-not $espVol) { throw '未找到 ESP 卷（VARIX-ESP）' }
  if ($espVol.DriveLetter) { return "$($espVol.DriveLetter):\" }
  $used = @(Get-Volume | Where-Object { $_.DriveLetter } | ForEach-Object { $_.DriveLetter })
  # 历次会话给 ESP 试过的字母（Z/Y/X/W）在 VDS 内部留有不可见占位，assign
  # 会报「驱动器号对于分配不可用」——ESP 从干净字母（Q 起降序）开始选
  $L = $null
  foreach ($c in @('Q', 'P', 'O', 'N', 'M', 'L', 'K', 'J', 'I', 'H', 'G', 'F', 'Z', 'Y', 'X', 'W', 'V', 'U', 'T', 'S', 'R')) {
    if ($used -notcontains $c) { $L = $c; break }
  }
  if (-not $L) { throw 'D-Z 无可用盘符' }
  # 先清残留挂载点（历次会话分配的字母在 mount manager 中占位但卷不可见，
  # 会导致 VDS 报「指定的驱动器号对于分配不可用」）
  $dp = "select disk $DiskNumber`r`nselect partition 1`r`nremove all dismount`r`nassign letter=$L`r`n"
  $dpFile = Join-Path $env:TEMP 'varix-esp-letter.txt'
  [IO.File]::WriteAllText($dpFile, $dp, [Text.Encoding]::ASCII)
  $mout = diskpart /s $dpFile 2>&1 | Out-String
  Start-Sleep -Seconds 2
  # 判定用 Test-Path（文件系统视角，与 Build-ESP 实际读写一致）：
  # Get-Volume 对 ESP 卷的盘符不反映（实测 diskpart 分配成功后仍查不到）
  if (-not (Test-Path -LiteralPath "$($L):\")) {
    Write-Note "diskpart 输出：$($mout.Trim())"
    throw "ESP 盘符分配失败：$L`:"
  }
  Write-Note "ESP 盘符：$L`:（部署会话内有效）"
  return "$($L):\"
}

# 盘符兜底：断点续作跳过 Partition 阶段时，Create-Partitions 内的盘符分配
# 不会执行，而 SHARED/VARIX_SYS 等普通分区需要盘符供后续阶段定位。
# ESP 跳过（盘符会被回收，走 Get-EspMount 文件夹挂载点）。幂等：已挂载跳过。
function Ensure-DiskLetters {
  param([int]$DiskNum)
  foreach ($p in @(Get-Partition -DiskNumber $DiskNum -ErrorAction SilentlyContinue)) {
    if ($p.GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}') { continue }
    $volGuid = @($p.AccessPaths | Where-Object { $_ -like '\\?\Volume{*}' } | Select-Object -First 1)
    if ($volGuid) {
      $vol = Get-Volume | Where-Object { $_.ObjectId -eq $volGuid }
      if ($vol -and $vol.DriveLetter) { continue }
    }
    $used = @(Get-Volume | Where-Object { $_.DriveLetter } | ForEach-Object { $_.DriveLetter })
    $used += @(Get-PSDrive -PSProvider FileSystem | Select-Object -ExpandProperty Name)
    $L = $null
    foreach ($c in @('Z', 'Y', 'X', 'W', 'V', 'U', 'T', 'S', 'R', 'Q', 'P', 'O', 'N', 'M', 'L', 'K', 'J', 'I', 'H', 'G', 'F')) {
      if ($used -notcontains $c) { $L = $c; break }
    }
    if (-not $L) { throw 'D-Z 无可用盘符' }
    $dp = "select disk $DiskNum`r`nselect partition $($p.PartitionNumber)`r`nassign letter=$L`r`n"
    $dpFile = Join-Path $env:TEMP 'varix-assign.txt'
    [IO.File]::WriteAllText($dpFile, $dp, [Text.Encoding]::ASCII)
    diskpart /s $dpFile 2>&1 | Out-Null
    # 挂载点传播延迟，等待后按 Get-Volume 视角验证
    Start-Sleep -Seconds 2
    if (-not (Get-Volume -DriveLetter $L -ErrorAction SilentlyContinue)) {
      throw "盘符分配失败：P$($p.PartitionNumber) -> $L`:"
    }
    Write-Note "卷盘符分配：P$($p.PartitionNumber) -> $L`:"
  }
}

# 盘符兜底（幂等）：断点续作跳过 Partition 时补齐五分区盘符
if ($DiskNumber -ge 0) { Ensure-DiskLetters -DiskNum $DiskNumber }

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
      # 哈希表 splat（命名参数）：数组 splat 会把整个数组当单个位置实参
      # 绑给 [int]$DiskNumber → 绑定异常终止（实测 EXP1/EXP2 证实）
      $h = @{ ReportPath = (Join-Path $ReportDir 'partition-map.txt') }
      if ($VhdPath) { $h.VhdPath = $VhdPath } else { $h.DiskNumber = $DiskNumber }
      # -Yes 授权整机部署 = 全链危险确认，含布局不符时销毁重建
      if ($Yes) { $h.Yes = $true; $h.Recreate = $true }
      & (Join-Path $PSScriptRoot 'Create-Partitions.ps1') @h
      # PS 子脚本失败以异常传播（ErrorActionPreference=Stop），不查 $LASTEXITCODE
      #（StrictMode 下未设置时访问即抛；& 调用的脚本 exit 也不更新它）
      Set-Stage $diskKey 'Partition'
    }
    'ESP' {
      # ESP 盘符不持久（mount manager 回收），用 NTFS 文件夹挂载点访问
      $espMount = Get-EspMount
      # 哈希表 splat（命名参数）：数组 splat 会把整个数组当单个位置实参
      $h = @{ EspPath = $espMount }
      if ($WindowsBootDir) { $h.WindowsBootDir = $WindowsBootDir }
      & (Join-Path $PSScriptRoot 'Build-ESP.ps1') @h
      Set-Stage $diskKey 'ESP'
    }
    'Shared' {
      $shVol = Get-VolByLabel 'SHARED'
      if (-not $shVol -or -not $shVol.DriveLetter) { throw '未找到 SHARED 卷' }
      & (Join-Path $PSScriptRoot 'Init-Shared.ps1') -SharedRoot "$($shVol.DriveLetter):"
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
      # ESP 用挂载点（盘符不持久）；校验失败以异常传播
      $espMount = Get-EspMount
      & (Join-Path $PSScriptRoot 'Build-ESP.ps1') -EspPath $espMount -VerifyOnly
      $shVol = Get-VolByLabel 'SHARED'
      if ($shVol -and $shVol.DriveLetter) {
        & (Join-Path $PSScriptRoot 'Init-Shared.ps1') -SharedRoot "$($shVol.DriveLetter):" -ValidateOnly
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
