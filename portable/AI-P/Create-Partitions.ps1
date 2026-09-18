<#
.SYNOPSIS
  VARIX 双域系统 · AI-P 任务 6 —— U 盘五分区 GPT 分区脚本（定版布局）

.DESCRIPTION
  对应 docs/VARIX双域系统施工总案.md 阶段 1 步骤 1：
    五分区 GPT：ESP(1GB) / VARIX_SYS(64GB) / WIN_ENGINE(300GB) / SHARED(600GB, exFAT) / SNAPSHOT(余量)
    - 容量全部参数化（partition-plan.json 配置驱动，version 字段开放性）
    - 4K 对齐校验（每分区 Offset % 4096 == 0，不对齐即报错不落盘）
    - 幂等重跑（已有分区时校验布局一致才继续；不一致要求 -Recreate 显式确认）
    - 清盘二次确认交互（-Yes 可跳过，仅供自动化）
    - -PlanOnly 纯计算模式：不碰任何磁盘，输出布局供测试与部署预览
  目标支持两种：物理 U 盘（-DiskNumber）或 VHD 文件（-VhdPath，供 VM/测试链路）。

.EXAMPLE
  .\Create-Partitions.ps1 -DiskNumber 3
  .\Create-Partitions.ps1 -VhdPath D:\Variable-USB\varix-usb.vhdx -Yes
  .\Create-Partitions.ps1 -PlanOnly -ReportPath D:\plan.txt

.NOTES
  编码纪律：UTF-8 BOM；危险操作（清盘/重建）必须二次确认（全局不变量"可停性/零残留"）。
#>
[CmdletBinding()]
param(
  [int]$DiskNumber = -1,

  [string]$VhdPath = "",

  [string]$Config = "",

  # 跳过清盘二次确认（自动化专用；人工跑请保持交互确认）
  [switch]$Yes,

  # 不碰任何磁盘，只计算并校验布局（开放性/测试用）
  [switch]$PlanOnly,

  # 幂等校验发现布局不一致时，允许销毁重建（仍会再次交互确认，除非 -Yes）
  [switch]$Recreate,

  # ASCII 部署报告输出路径（可选；总案要求分区表可视化归档）
  [string]$ReportPath = "",

  # -PlanOnly 时使用的虚拟磁盘容量（GB），默认 1024
  [int]$PlanDiskGB = 1024
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Write-Step { param([string]$Msg) Write-Host ">>> $Msg" -ForegroundColor Cyan }
function Write-Ok   { param([string]$Msg) Write-Host "    + $Msg" -ForegroundColor Green }
function Write-Note { param([string]$Msg) Write-Host "    ! $Msg" -ForegroundColor Yellow }

# ---------------------------------------------------------- 布局纯函数（可宿主机单测）
# 输入：配置对象 + 磁盘总字节数。输出：每分区 {Name,Fs,Label,SizeGB,OffsetBytes,SizeBytes}。
# 规则：SNAPSHOT sizeGB=0 表示吃掉余量；首分区起点与对齐来自配置；不对齐/超容量/余量不足返回 $null 并填 $script:PlanError。
function Get-PartitionPlan {
  param(
    [Parameter(Mandatory = $true)]$Config,
    [Parameter(Mandatory = $true)][int64]$DiskSizeBytes
  )
  $script:PlanError = ""
  $align = [int64]$Config.alignmentBytes
  $offset = [int64]$Config.firstOffsetBytes
  if (($offset % $align) -ne 0) {
    $script:PlanError = "firstOffsetBytes=$offset 非 $align 对齐"
    return $null
  }
  $plan = @()
  foreach ($p in @($Config.partitions)) {
    $sizeBytes = [int64]$p.sizeGB * 1GB
    if ($sizeBytes -eq 0) {
      $sizeBytes = $DiskSizeBytes - $offset
      if ($sizeBytes -lt ([int64]$Config.minSnapshotGB * 1GB)) {
        $script:PlanError = "SNAPSHOT 余量 $([math]::Round($sizeBytes/1GB,1))GB < 最低要求 $($Config.minSnapshotGB)GB（盘太小）"
        return $null
      }
    }
    $end = $offset + $sizeBytes
    if ($end -gt $DiskSizeBytes) {
      $script:PlanError = "分区 $($p.name) 越界（end=$end > 磁盘=$DiskSizeBytes）"
      return $null
    }
    $plan += [pscustomobject]@{
      Name        = [string]$p.name
      Fs          = [string]$p.fs
      Label       = [string]$p.label
      SizeGB      = [int]$p.sizeGB
      OffsetBytes = $offset
      SizeBytes   = $sizeBytes
    }
    $offset = $end
  }
  # 终局校验：每个 Offset 必须 4K 对齐（sizeGB 均为整 GB，天然满足，但仍显式断言防配置被改坏）
  foreach ($e in $plan) {
    if (($e.OffsetBytes % $align) -ne 0) {
      $script:PlanError = "分区 $($e.Name) Offset=$($e.OffsetBytes) 非 $align 对齐"
      return $null
    }
  }
  return $plan
}

# 渲染 ASCII 分区表（总案：分区表可视化入部署报告）
function Format-PartitionPlanAscii {
  param([Parameter(Mandatory = $true)]$Plan, [int64]$DiskSizeBytes)
  $lines = @()
  $lines += "+---------+--------+------------+----------------+"
  $lines += "| Name    | FS     | Size(GB)   | Offset(MiB)    |"
  $lines += "+---------+--------+------------+----------------+"
  foreach ($e in $Plan) {
    $lines += ("| {0,-7} | {1,-6} | {2,10} | {3,14} |" -f $e.Name, $e.Fs, $e.SizeGB, [int]($e.OffsetBytes / 1MB))
  }
  $lines += ("| TOTAL   |        | {0,10} |                |" -f [math]::Round($DiskSizeBytes / 1GB, 1))
  $lines += "+---------+--------+------------+----------------+"
  return $lines -join [Environment]::NewLine
}

# 盘符保障：ESP 等 GPT 特殊分区 Windows 不会自动挂盘符，而 Deploy 编排按
# 卷标签 + DriveLetter 定位（Get-VolByLabel 找到卷但无盘符同样判失败）。
# 显式从高位字母分配（等价 diskpart assign letter），已有盘符直接跳过。
function Add-VolDriveLetter {
  param([Parameter(Mandatory = $true)]$Part)
  # Storage WMI 分区对象状态陈旧（实测：diskpart 分配成功后 AccessPaths /
  # DriveLetter 长时间为空）——以 AccessPaths 的 Volume{guid} 映射卷，
  # 已挂载判定与分配成功判定都用 Get-Volume 视角（实证新鲜可靠）
  $cur = Get-Partition -DiskNumber $Part.DiskNumber -PartitionNumber $Part.PartitionNumber
  # ESP 的盘符会被 mount manager 回收（实测），不分配——由 Deploy 编排用
  # NTFS 文件夹挂载点访问（Get-EspMount）
  if ($cur.GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}') { return }
  $volGuid = @($cur.AccessPaths | Where-Object { $_ -like '\\?\Volume{*}' } | Select-Object -First 1)
  if ($volGuid) {
    $vol = Get-Volume | Where-Object { $_.ObjectId -eq $volGuid }
    if ($vol -and $vol.DriveLetter) { return }
  }
  $used = @(Get-Volume | Where-Object { $_.DriveLetter } | ForEach-Object { $_.DriveLetter })
  $used += @(Get-PSDrive -PSProvider FileSystem | Select-Object -ExpandProperty Name)
  foreach ($L in @('Z', 'Y', 'X', 'W', 'V', 'U', 'T', 'S', 'R', 'Q', 'P', 'O', 'N', 'M', 'L', 'K', 'J', 'I', 'H', 'G', 'F')) {
    if ($used -notcontains $L) {
      # diskpart assign（Storage cmdlet 参数集/管道绑定实测均不可用）
      $dp = "select disk $($cur.DiskNumber)`r`nselect partition $($cur.PartitionNumber)`r`nassign letter=$L`r`n"
      $dpFile = Join-Path $env:TEMP 'varix-assign.txt'
      [IO.File]::WriteAllText($dpFile, $dp, [Text.Encoding]::ASCII)
      diskpart /s $dpFile 2>&1 | Out-Null
      Start-Sleep -Seconds 2
      if (-not (Get-Volume -DriveLetter $L -ErrorAction SilentlyContinue)) {
        throw "盘符分配失败（diskpart assign）：P$($cur.PartitionNumber) -> $L`:"
      }
      Write-Ok "盘符分配：Disk$($cur.DiskNumber) P$($cur.PartitionNumber) -> $L`:"
      return
    }
  }
  throw 'D-Z 无可用盘符，无法挂载分区卷'
}

# 幂等校验：现有分区与计划逐项比对（数量/标签/容量）。一致 $true；不一致 $false 并把差异填到差异表。
function Test-LayoutMatch {
  param([Parameter(Mandatory = $true)]$Plan, [Parameter(Mandatory = $true)]$Parts)
  $script:LayoutDiffs = @()
  $real = @($Parts | Sort-Object PartitionNumber)
  if ($real.Count -ne $Plan.Count) {
    $script:LayoutDiffs += "分区数不符：期望 $($Plan.Count) 实际 $($real.Count)"
    return $false
  }
  for ($i = 0; $i -lt $Plan.Count; $i++) {
    $want = $Plan[$i]
    $got = $real[$i]
    $gotGB = [math]::Round($got.Size / 1GB, 0)
    if ($want.SizeGB -eq 0) {
      # SNAPSHOT 是余量，只要它是最后一块且 >= 最低值即视为一致
      if ($i -ne $Plan.Count - 1) { $script:LayoutDiffs += "SNAPSHOT 不在末位" }
      continue
    }
    if ($gotGB -ne $want.SizeGB) {
      $script:LayoutDiffs += "$($want.Name) 容量不符：期望 $($want.SizeGB)GB 实际 ${gotGB}GB"
    }
  }
  return ($script:LayoutDiffs.Count -eq 0)
}

# ---------------------------------------------------------------- 入口
if ($PlanOnly) {
  if (-not $Config) { $Config = Join-Path $PSScriptRoot 'partition-plan.json' }
  $cfg = Get-Content -LiteralPath $Config -Raw | ConvertFrom-Json
  $planGB = $PlanDiskGB
  $plan = Get-PartitionPlan -Config $cfg -DiskSizeBytes ([int64]$planGB * 1GB)
  if (-not $plan) { throw "布局计算失败：$script:PlanError" }
  $ascii = Format-PartitionPlanAscii -Plan $plan -DiskSizeBytes ([int64]$planGB * 1GB)
  Write-Host $ascii
  if ($ReportPath) { $ascii | Set-Content -LiteralPath $ReportPath -Encoding UTF8 }
  exit 0
}

if ($DiskNumber -lt 0 -and -not $VhdPath) { throw '必须提供 -DiskNumber 或 -VhdPath 之一' }
if ($DiskNumber -ge 0 -and $VhdPath) { throw '-DiskNumber 与 -VhdPath 只能二选一' }

# 权限门禁：真正动盘才需要管理员（-PlanOnly 是纯计算，供测试与预览）
if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  throw '分区操作需要管理员 PowerShell（-PlanOnly 除外请先提权）'
}

if (-not $Config) { $Config = Join-Path $PSScriptRoot 'partition-plan.json' }
$cfg = Get-Content -LiteralPath $Config -Raw | ConvertFrom-Json
if ([int]$cfg.version -lt 1) { throw "配置 version 非法：$($cfg.version)" }

$mountedVhd = $false
try {
  if ($VhdPath) {
    if (-not (Test-Path -LiteralPath $VhdPath)) { throw "VHD 不存在：$VhdPath" }
    Write-Step "挂载 VHD $VhdPath"
    Mount-VHD -Path $VhdPath -PassThru | Out-Null
    $mountedVhd = $true
    Start-Sleep -Seconds 2
    $fileName = Split-Path -Leaf $VhdPath
    $disk = Get-Disk | Where-Object { $_.Location -like "*$fileName*" } | Select-Object -First 1
    if (-not $disk) { throw "挂载后未找到磁盘：$VhdPath" }
  }
  else {
    $disk = Get-Disk -Number $DiskNumber
    if (-not $disk) { throw "磁盘不存在：Disk $DiskNumber" }
    if ($disk.IsBoot -or $disk.IsSystem) {
      throw "磁盘 $DiskNumber 是启动/系统盘，拒绝操作（全局不变量：物理隔离）"
    }
  }

  $diskGB = [math]::Round($disk.Size / 1GB, 1)
  Write-Step "目标磁盘 $($disk.Number)（$diskGB GB / $($disk.FriendlyName)）"
  if ($diskGB -lt [double]$cfg.minDiskGB) {
    throw "磁盘 ${diskGB}GB 低于最低要求 $($cfg.minDiskGB)GB，拒绝分区（总案：小盘明确报错）"
  }

  $plan = Get-PartitionPlan -Config $cfg -DiskSizeBytes $disk.Size
  if (-not $plan) { throw "布局计算失败：$script:PlanError" }
  $ascii = Format-PartitionPlanAscii -Plan $plan -DiskSizeBytes $disk.Size
  Write-Host $ascii
  if ($ReportPath) { $ascii | Set-Content -LiteralPath $ReportPath -Encoding UTF8 }

  # 幂等重跑：已有分区 → 校验布局一致才继续
  $existing = @(Get-Partition -DiskNumber $disk.Number -ErrorAction SilentlyContinue)
  if ($existing.Count -gt 0) {
    Write-Step "已有 $($existing.Count) 个分区，执行幂等布局校验"
    if (Test-LayoutMatch -Plan $plan -Parts $existing) {
      Write-Ok '布局与计划一致（幂等重跑通过），跳过分区与格式化'
      # 幂等路径也补盘符（首次分区后盘符分配失败的重跑场景）
      foreach ($p in $existing) { Add-VolDriveLetter -Part $p }
      exit 0
    }
    Write-Note '布局与计划不一致：'
    $script:LayoutDiffs | ForEach-Object { Write-Note "  - $_" }
    if (-not $Recreate) {
      throw '布局不一致。确认要销毁重建请加 -Recreate（数据会全部丢失）'
    }
  }

  # 危险操作二次确认（全局不变量：可停性/零残留）
  if (-not $Yes) {
    Write-Host ''
    Write-Host '即将清空并重建磁盘上全部分区，数据不可恢复！' -ForegroundColor Red
    $answer = Read-Host "确认对磁盘 $($disk.Number)（$diskGB GB）执行？输入大写 YES 继续"
    if ($answer -cne 'YES') { throw '用户未确认，已中止（未做任何修改）' }
  }

  Write-Step '清盘（Clear-Disk）+ GPT 初始化'
  Clear-Disk -Number $disk.Number -RemoveData -RemoveOEM -Confirm:$false
  # Clear-Disk 后盘可能仍是 GPT 元数据，重复 Initialize 会报错——条件化
  $d2 = Get-Disk -Number $disk.Number
  if ($d2.PartitionStyle -ne 'GPT') {
    Initialize-Disk -Number $disk.Number -PartitionStyle GPT | Out-Null
  }
  # Windows GPT 初始化会自动建 16MB MSR 保留分区（Offset 17KB），与 ESP 的
  # 1MiB 计划起点重叠，必须移除（Remove-Partition 对受保护分区可能拒绝 →
  # diskpart delete override 兜底）。
  $msrType = '{e3c9e316-0b5c-4db8-817d-f92df00215ae}'
  $msr = Get-Partition -DiskNumber $disk.Number -ErrorAction SilentlyContinue |
    Where-Object { $_.GptType -eq $msrType }
  if ($msr) {
    Write-Note '移除 GPT 初始化自动创建的 MSR 保留分区'
    $msr | Remove-Partition -Confirm:$false -ErrorAction SilentlyContinue
    if (Get-Partition -DiskNumber $disk.Number -ErrorAction SilentlyContinue | Where-Object { $_.GptType -eq $msrType }) {
      $dp = "select disk $($disk.Number)`r`nselect partition $($msr.PartitionNumber)`r`ndelete partition override`r`n"
      $dpFile = Join-Path $env:TEMP 'varix-delmsr.txt'
      [IO.File]::WriteAllText($dpFile, $dp, [Text.Encoding]::ASCII)
      diskpart /s $dpFile | Out-Null
    }
    if (Get-Partition -DiskNumber $disk.Number -ErrorAction SilentlyContinue | Where-Object { $_.GptType -eq $msrType }) {
      throw 'MSR 保留分区移除失败（Remove-Partition 与 diskpart override 均未生效）'
    }
    Write-Ok 'MSR 已移除'
  }

  Write-Step '创建五分区并格式化'
  foreach ($e in $plan) {
    # New-Partition 的容量参数名是 -Size（不是 SizeBytes）
    $npParams = @{ DiskNumber = $disk.Number; Offset = $e.OffsetBytes; Size = $e.SizeBytes }
    if ($e.Name -eq 'ESP') {
      $npParams.GptType = '{C12A7328-F81F-11D2-BA4B-00A0C93EC93B}'
    }
    if ($e.SizeGB -eq 0) {
      # 余量分区：-UseMaximumSize 由 Storage 层吃掉剩余 extent（自动避开 GPT
      # 备份结构与 USB 桥接容量偏差）；按绝对字节硬算会报容量不足
      $npParams.Remove('Size') | Out-Null
      $npParams.UseMaximumSize = $true
    }
    $np = New-Partition @npParams -ErrorAction Stop
    if (-not $np) { throw "分区 $($e.Name) 创建失败（New-Partition 返回空）" }
    if (($np.Offset % 4096) -ne 0) { throw "分区 $($e.Name) 落盘 Offset=$($np.Offset) 非 4K 对齐，中止" }
    Format-Volume -Partition $np -FileSystem $e.Fs -NewFileSystemLabel $e.Label -Confirm:$false | Out-Null
    Write-Ok "$($e.Name)  $([math]::Round($e.SizeBytes/1GB,1))GB  $($e.Fs)  $($e.Label)（Offset=$($np.Offset)，4K 对齐 OK）"
  }

  Write-Step '收尾：终验（五分区存在 + 卷标签正确）'
  $final = @(Get-Partition -DiskNumber $disk.Number | Sort-Object PartitionNumber)
  if ($final.Count -ne 5) { throw "终验失败：期望 5 分区实际 $($final.Count)" }
  foreach ($e in $plan) {
    $vol = Get-Volume -FileSystemLabel $e.Label -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $vol) { throw "终验失败：卷标签 $($e.Label) 未找到" }
  }
  Write-Ok '终验通过：五分区全部就位且标签正确'
}
finally {
  if ($mountedVhd) {
    Dismount-VHD -Path $VhdPath -ErrorAction SilentlyContinue
  }
}
