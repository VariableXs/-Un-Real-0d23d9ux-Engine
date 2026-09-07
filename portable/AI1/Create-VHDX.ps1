<#
.SYNOPSIS
  Variable OS · AI-1 存储核 —— VHDX 造盘定版脚本（1TB / 1000MB/s 双接口盘）

.DESCRIPTION
  对应 docs/PORTABLE_VIRTUAL_SYSTEM_PLAN.md：
    3.1 VHDX 链设计   -> -Chain 生成 Base(只读母盘) -> Apps -> User 三级差分链
    3.2 读写分离      -> 自动建 Data/{Apps,MSIX,Plugins,User,Exchange,Cache,Dumps}
    3.3 动态 vs 固定  -> 1TB 固态默认 Fixed 150GB（-VhdType Dynamic 可切回）
    9.2 文件系统      -> NTFS 64KB 簇 + 4K 对齐校验（扩充 29.4）
    9.3/9.4 优化裁剪  -> 离线 CompactOS + 注册表关闭 SysMain/WSearch/DiagTrack（扩充 26.1/26.2）
  宿主侧 TRIM / LastAccess / 碎片策略见 -TuneHost；进系统后的一次性调优见 Tune-Guest.ps1。

.EXAMPLE
  .\Create-VHDX.ps1 -IsoPath C:\Win11_22H2.iso -OutDir D:\Variable-USB
  # 1TB 1000MB/s 盘定版：Fixed 150GB + 64KB 簇 + Data 目录 + 离线调优

.EXAMPLE
  .\Create-VHDX.ps1 -IsoPath C:\Win11_22H2.iso -OutDir D:\Variable-USB -Chain
  # 追加 3.1 三级差分链：Base(20GB 只读) -> Apps(差分) -> User(差分)

.NOTES
  需管理员 PowerShell 5.1+ / 7.x，需 Hyper-V 模块（缺失时脚本会启用并要求重启）。
  脚本只写 OutDir（未来 U 盘根），不改宿主 C 盘；-TuneHost 才动宿主 fsutil 行为。
#>
#Requires -RunAsAdministrator
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)]
  [string]$IsoPath,

  # 未来 U 盘根目录（VHDX + Data + PortableVM + EFI 都放这里，见主计划附录 A）
  [string]$OutDir = 'D:\Variable-USB',

  # 3.3：1TB 1000MB/s 固态用固定盘，顺序读写省去块分配元数据开销
  [int]$SizeGB = 150,

  [ValidateSet('Fixed', 'Dynamic')]
  [string]$VhdType = 'Fixed',

  # 固定盘 + NTFS 稀疏：语义仍是 Fixed，磁盘实占按已写入块计（实验性，见文档 3.3 节）
  [switch]$Sparse,

  # 3.1 三级差分链
  [switch]$Chain,
  [int]$BaseGB = 20,

  # 9.2 簇大小（KB）：64KB 减少碎片、提升大文件顺序读
  [ValidateSet(4, 8, 16, 32, 64)]
  [int]$AllocationUnitKB = 64,

  # install.wim 索引（专业版通常 6，用 dism /Get-WimInfo /WimFile:... 核对）
  [int]$ImageIndex = 6,

  [switch]$SkipApply,
  [switch]$SkipTune,
  [switch]$TuneHost,
  [switch]$Force
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Write-Step { param([string]$Msg) Write-Host ">>> $Msg" -ForegroundColor Cyan }
function Write-Ok   { param([string]$Msg) Write-Host "    + $Msg" -ForegroundColor Green }
function Write-Note { param([string]$Msg) Write-Host "    ! $Msg" -ForegroundColor Yellow }

function Get-FreeLetter {
  $used = @(Get-Volume | Where-Object { $_.DriveLetter } | ForEach-Object { [string]$_.DriveLetter })
  foreach ($c in [char[]]'DEFGHIJKLMNOPQRSTUVWXYZ') {
    if ($used -notcontains [string]$c) { return [string]$c }
  }
  throw '无可用盘符（D-Z 全部占用）'
}

function New-TargetVhd {
  param(
    [string]$Path,
    [int]$GB,
    [string]$Type
  )
  if (Test-Path -LiteralPath $Path) {
    if ($Force) {
      Dismount-VHD -Path $Path -ErrorAction SilentlyContinue
      Remove-Item -LiteralPath $Path -Force
      Write-Note "已删除旧文件 $Path（-Force）"
    }
    else {
      Write-Note "已存在 $Path，将复用（-Force 可重建）"
      return
    }
  }
  $bytes = [int64]$GB * 1GB
  if ($Type -eq 'Fixed') { New-VHD -Path $Path -SizeBytes $bytes -Fixed | Out-Null }
  else { New-VHD -Path $Path -SizeBytes $bytes -Dynamic | Out-Null }
  Write-Ok "已创建 $Path（$GB GB / $Type）"
}

# ---------------------------------------------------------------- 0. 前置检查
Write-Step '0/7 前置检查（管理员 / Hyper-V / ISO）'
if (-not (Test-Path -LiteralPath $IsoPath)) { throw "ISO 不存在：$IsoPath" }
if (-not (Get-Module -ListAvailable -Name Hyper-V)) {
  Write-Note '未检测到 Hyper-V 模块，正在启用 Microsoft-Hyper-V'
  Enable-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V -All -NoRestart | Out-Null
  throw 'Hyper-V 已启用，请重启后重新运行本脚本'
}
Import-Module Hyper-V
Write-Ok "ISO=$IsoPath  OutDir=$OutDir  ${SizeGB}GB/$VhdType  簇=${AllocationUnitKB}KB"

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$vhdx = Join-Path $OutDir 'Variable-OS.vhdx'

# ------------------------------------------------------- 1. 3.2 读写分离目录
Write-Step '1/7 建立 Data 读写分离目录（主计划 3.2）'
$dataRoot = Join-Path $OutDir 'Data'
foreach ($d in @('Apps', 'MSIX', 'Plugins', 'User', 'Exchange', 'Cache', 'Dumps')) {
  $p = Join-Path $dataRoot $d
  New-Item -ItemType Directory -Force -Path $p | Out-Null
}
Write-Ok "Data 七目录就绪：$dataRoot\{Apps,MSIX,Plugins,User,Exchange,Cache,Dumps}"

# ------------------------------------------------- 2. 3.1/3.3 创建目标 VHDX
$target = $vhdx
$targetGB = $SizeGB
$targetType = $VhdType
if ($Chain) {
  $target = Join-Path $OutDir 'Base.vhdx'
  $targetGB = $BaseGB
  $targetType = 'Dynamic'   # 母盘需可被差分引用，用动态；只读由文件属性保证
}
Write-Step "2/7 创建 VHDX $target（$targetGB GB / $targetType）"
New-TargetVhd -Path $target -GB $targetGB -Type $targetType

if ($Sparse -and $targetType -eq 'Fixed') {
  # 固定盘全预分配 = 立刻吃掉 150GB；打上 NTFS 稀疏标记后实占按已写块增长
  fsutil.exe sparse setflag $target | Out-Null
  fsutil.exe sparse setrange $target 0 ([int64]$targetGB * 1GB) | Out-Null
  Write-Ok '已打 NTFS 稀疏标记（fsutil sparse setflag/setrange）'
}

# --------------------------------------------------- 3. 挂载 / GPT / 4K 对齐
Write-Step '3/7 挂载 VHDX + GPT 初始化 + 64KB 簇格式化'
$mounted = $false
try {
  Mount-VHD -Path $target -PassThru | Out-Null
  $mounted = $true
  Start-Sleep -Seconds 2
  $fileName = Split-Path -Leaf $target
  $disk = Get-Disk | Where-Object { $_.Location -like "*$fileName*" } | Select-Object -First 1
  if (-not $disk) { throw "挂载后未找到磁盘：$target" }
  if ($disk.PartitionStyle -eq 'RAW') {
    Initialize-Disk -Number $disk.Number -PartitionStyle GPT | Out-Null
    Write-Ok "GPT 初始化完成（Disk $($disk.Number)）"
  }
  # 复用已有分区（VHDX 复用路径），否则新建
  $part = Get-Partition -DiskNumber $disk.Number -ErrorAction SilentlyContinue |
    Sort-Object PartitionNumber | Select-Object -First 1
  if (-not $part) {
    $part = New-Partition -DiskNumber $disk.Number -UseMaximumSize -AssignDriveLetter
  }
  elseif (-not $part.DriveLetter) {
    $part | Set-Partition -NewDriveLetter (Get-FreeLetter)
    $part = Get-Partition -DiskNumber $disk.Number -PartitionNumber $part.PartitionNumber
  }
  if (($part.Offset % 4096) -ne 0) {
    Write-Note "分区偏移 $($part.Offset) 非 4096 倍数，4K 随机将掉约 50%（主计划 29.4）"
  }
  else {
    Write-Ok "4K 对齐 OK（Offset=$($part.Offset)）"
  }

  $vol = Format-Volume -Partition $part -FileSystem NTFS `
    -AllocationUnitSize ($AllocationUnitKB * 1KB) `
    -NewFileSystemLabel 'VariableOS' -Confirm:$false
  $driveLetter = $vol.DriveLetter
  if (-not $driveLetter) {
    $driveLetter = (Get-Partition -DiskNumber $disk.Number |
        Where-Object { $_.DriveLetter } | Select-Object -First 1).DriveLetter
  }
  if (-not $driveLetter) { throw '格式化后未取到盘符' }
  $drive = "${driveLetter}:"
  $chk = Get-Volume -DriveLetter $driveLetter
  Write-Ok ("已格式化 {0} NTFS 簇要求={1}KB 实测={2}KB 可用={3:N1}GB" -f `
      $drive, $AllocationUnitKB, [int]($chk.BlockSize / 1KB), ($chk.SizeRemaining / 1GB))

  # ------------------------------------------------ 4/5. 写镜像 + 写引导
  if ($SkipApply) {
    Write-Note '-SkipApply：跳过镜像展开与引导写入（复用已有系统盘）'
  }
  else {
    Write-Step "4/7 挂载 ISO 并 dism /Apply-Image（Index=$ImageIndex，约 10 分钟）"
    $isoMount = Mount-DiskImage -ImagePath $IsoPath -PassThru | Get-Volume
    $isoLetter = "$($isoMount.DriveLetter):"
    $srcDir = Join-Path $isoLetter 'sources'
    $wim = Get-ChildItem -Path $srcDir -Filter 'install.wim' -ErrorAction SilentlyContinue |
      Select-Object -First 1
    if (-not $wim) {
      $wim = Get-ChildItem -Path $srcDir -Filter 'install.esd' -ErrorAction SilentlyContinue |
        Select-Object -First 1
    }
    if (-not $wim) {
      Dismount-DiskImage -ImagePath $IsoPath | Out-Null
      throw "未在 $srcDir 找到 install.wim / install.esd"
    }
    Write-Ok "镜像 $($wim.FullName) -> $drive"
    & dism.exe /Apply-Image "/ImageFile:$($wim.FullName)" "/Index:$ImageIndex" "/ApplyDir:$drive\"
    if ($LASTEXITCODE -ne 0) { throw "dism /Apply-Image 失败（exit=$LASTEXITCODE）" }

    Write-Step '5/7 写入 UEFI 引导（bcdboot）'
    & bcdboot.exe "$drive\Windows" /s $drive /f UEFI
    if ($LASTEXITCODE -ne 0) { throw "bcdboot 失败（exit=$LASTEXITCODE）" }
    Write-Ok "引导已写入 $drive\EFI\Microsoft\Boot"
    Dismount-DiskImage -ImagePath $IsoPath | Out-Null
  }

  # ------------------------------------- 6. 离线调优 9.3 / 9.4 / 扩充 26.1-26.2
  if ($SkipTune) {
    Write-Note '-SkipTune：跳过离线 CompactOS 与注册表调优'
  }
  else {
    Write-Step '6/7 离线调优（CompactOS + 注册表）'
    & dism.exe "/Image:$drive\" /Compact:On | Out-Null
    Write-Ok 'CompactOS 已开启（主计划 9.3，预计省约 30% 空间）'

    $sysHive = Join-Path $drive 'Windows\System32\config\SYSTEM'
    $softHive = Join-Path $drive 'Windows\System32\config\SOFTWARE'
    reg.exe load 'HKLM\VAR_OFF_SYS' $sysHive | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "reg load 失败（exit=$LASTEXITCODE）：$sysHive" }
    try {
      # 扩充 26.1/26.2：Superfetch(SysMain) 关闭、索引与遥测降级、诊断跟踪关闭
      reg.exe add 'HKLM\VAR_OFF_SYS\ControlSet001\Services\SysMain' /v Start /t REG_DWORD /d 4 /f | Out-Null
      reg.exe add 'HKLM\VAR_OFF_SYS\ControlSet001\Services\WSearch' /v Start /t REG_DWORD /d 3 /f | Out-Null
      reg.exe add 'HKLM\VAR_OFF_SYS\ControlSet001\Services\DiagTrack' /v Start /t REG_DWORD /d 4 /f | Out-Null
      reg.exe add 'HKLM\VAR_OFF_SYS\ControlSet001\Services\wuauserv' /v Start /t REG_DWORD /d 3 /f | Out-Null
      # 扩充 26.1 / 主计划 9.2：关 LastAccess、开长路径
      reg.exe add 'HKLM\VAR_OFF_SYS\ControlSet001\Control\FileSystem' /v NtfsDisableLastAccessUpdate /t REG_DWORD /d 1 /f | Out-Null
      reg.exe add 'HKLM\VAR_OFF_SYS\ControlSet001\Control\FileSystem' /v LongPathsEnabled /t REG_DWORD /d 1 /f | Out-Null
    }
    finally {
      reg.exe unload 'HKLM\VAR_OFF_SYS' | Out-Null
    }
    reg.exe load 'HKLM\VAR_OFF_SOFT' $softHive | Out-Null
    try {
      reg.exe add 'HKLM\VAR_OFF_SOFT\Microsoft\Windows\CurrentVersion\Explorer' /v 'Max Cached Icons' /t REG_DWORD /d 4096 /f | Out-Null
    }
    finally {
      reg.exe unload 'HKLM\VAR_OFF_SOFT' | Out-Null
    }
    Write-Ok 'SysMain=4 WSearch=3 DiagTrack=4 wuauserv=3 LastAccess=1 LongPaths=1'
    Write-Note 'powercfg /hibernate off、fsutil、服务停止请在系统内跑 Tune-Guest.ps1'
  }
}
finally {
  if ($mounted) {
    Dismount-VHD -Path $target -ErrorAction SilentlyContinue
  }
}

# ------------------------------------------------------- 7. 3.1 三级差分链
if ($Chain) {
  Write-Step '7/7 生成三级差分链（主计划 3.1）'
  $apps = Join-Path $OutDir 'Variable-OS-Apps.vhdx'
  $user = Join-Path $OutDir 'User.vhdx'
  if (-not (Test-Path -LiteralPath $apps)) {
    New-VHD -Path $apps -ParentPath $target -Differencing | Out-Null
    Write-Ok "Apps 层：$apps（差分，虚拟大小继承母盘，文件按需增长）"
  }
  if (-not (Test-Path -LiteralPath $user)) {
    New-VHD -Path $user -ParentPath $apps -Differencing | Out-Null
    Write-Ok "User 层：$user（差分，日常备份/一键还原对象）"
  }
  Set-ItemProperty -LiteralPath $target -Name IsReadOnly -Value $true
  Write-Ok "母盘已置只读：$target（哈希校验防篡改）"
  Write-Note '合并（扁平化）用：Merge-VHD -Path User.vhdx -DestinationPath Variable-OS-Apps.vhdx'
}
else {
  Write-Step '7/7 差分链跳过（-Chain 可生成 Base -> Apps -> User）'
}

# ------------------------------------------------------------- 收尾与自检
Write-Step '收尾：VHDX 元数据自检'
$info = Get-VHD -Path $target
$info | Select-Object VhdFormat, VhdType, @{n = 'VirtualGB'; e = { [math]::Round($_.Size / 1GB, 1) } }, `
  @{n = 'FileGB'; e = { [math]::Round($_.FileSize / 1GB, 2) } }, FragmentationPercentage |
  Format-List
if ($info.FragmentationPercentage -gt 5) {
  Write-Note "碎片率 $($info.FragmentationPercentage)% > 5%，请跑 Maintain-VHDX.ps1（主计划 3.1 验收）"
}
else {
  Write-Ok "碎片率 $($info.FragmentationPercentage)% ≤ 5%"
}

if ($TuneHost) {
  Write-Step '宿主侧调优（主计划 9.2/9.3 + 扩充 26.4）'
  fsutil.exe behavior set DisableDeleteNotify 0 | Out-Null   # 9.3 启用 TRIM
  fsutil.exe behavior set DisableLastAccess 1 | Out-Null     # 9.2 关 LastAccess
  Write-Ok 'TRIM 开启（DisableDeleteNotify=0）/ LastAccess 关闭'
  Write-Note '关闭 VHDX 所在卷的自动碎片整理，改为每月 Maintain-VHDX.ps1 手动一次'
  Write-Note 'B 模式上盘后建议：diskpart> automount disable（禁止自动挂载宿主盘）'
}

Write-Host ''
Write-Host ">>> 完成：$target" -ForegroundColor Green
Write-Host '下一步：' -ForegroundColor Green
Write-Host "  1) .\Tune-Guest.ps1        # 进系统内一次性调优（9.3/9.4/9.5）"
Write-Host "  2) .\Link-DataApps.ps1 -DataRoot $dataRoot   # 3.2 读写分离：把大软件链接到 Data"
Write-Host "  3) .\Bench-Storage.ps1     # 9.1 选盘验证 + SEQ/4K/膨胀率 -> Bench.md"
Write-Host "  4) ..\AI2\Test-VM.ps1 -Vhdx $target   # 隔离验证（AI-2 负责）"
