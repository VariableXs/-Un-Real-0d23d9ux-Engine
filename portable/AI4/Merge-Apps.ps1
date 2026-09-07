param(
  [ValidateSet("Merge", "Create", "Status", "Add-PortableApp")]
  [string]$Action = "Status",
  [string]$AppsVhdx   = "D:\Variable-USB\Apps.vhdx",
  [string]$BaseVhdx   = "D:\Variable-USB\Base.vhdx",
  [string]$UserVhdx   = "D:\Variable-USB\User.vhdx",
  [int]$AppsSizeGB    = 50,
  [string]$AppName    = "",
  [string]$AppSource  = "",
  [string]$DataRoot   = "D:\Data",
  [switch]$Force,
  [switch]$DryRun
)
# AI-4 拓展核 / 第8.1章 层式VHDX + 扩充14 插件/云
# 三层差分链: Base(20GB只读) <- Apps(50GB只读/共享) <- User(动态差分)
# 用法:
#   .\Merge-Apps.ps1 -Action Create -AppsSizeGB 50
#   .\Merge-Apps.ps1 -Action Merge            # Apps层合并回Base, 生成新Base
#   .\Merge-Apps.ps1 -Action Add-PortableApp -AppName Blender -AppSource D:\dl\Blender
#   .\Merge-Apps.ps1 -Action Status
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Test-Admin {
  return ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Test-StorageLayer {
  param([string]$Path)
  if (Test-Path $Path -PathType Leaf) {
    $item = Get-Item $Path -Force
    return $item.LinkType -eq "SymbolicLink" -or $item.Attributes -band [IO.FileAttributes]::ReparsePoint
  }
  return $false
}

function New-AppLayer {
  if (-not (Test-Path (Split-Path $AppsVhdx -Parent))) {
    New-Item -ItemType Directory -Force -Path (Split-Path $AppsVhdx -Parent) | Out-Null
  }
  if (Test-Path $AppsVhdx) {
    if (-not $Force) { throw "Apps.vhdx 已存在, 使用 -Force 覆盖或先 Merge" }
    Remove-VHD -Path $AppsVhdx -Confirm:$false
  }
  Write-Host ">>> 创建 Apps 层 $AppsVhdx ($AppsSizeGB GB, 动态)" -ForegroundColor Cyan
  New-VHD -Path $AppsVhdx -SizeBytes ($AppsSizeGB * 1GB) -Dynamic | Out-Null
  Mount-VHD -Path $AppsVhdx | Out-Null
  $disk = Get-Disk | Where-Object { $_.Location -like "*$([IO.Path]::GetFileName($AppsVhdx))*" } | Select-Object -First 1
  $part = Get-Partition -DiskNumber $disk.Number | Select-Object -First 1
  if (-not $part) {
    Initialize-Disk -Number $disk.Number -PartitionStyle GPT -PassThru | Out-Null
    $part = New-Partition -DiskNumber $disk.Number -UseMaximumSize -AssignDriveLetter
  }
  if (-not $part.DriveLetter) {
    throw "Apps.vhdx 分区未获得盘符"
  }
  Format-Volume -Partition $part -FileSystem NTFS -NewFileSystemLabel "VariableApps" -Confirm:$false -AllocationUnitSize 64KB | Out-Null
  $drive = $part.DriveLetter + ":"
  Write-Host ">>> Apps 层就绪 $drive (64KB簇)" -ForegroundColor Green
  Write-Host "    将绿色软件放入 $drive\Apps\, 然后运行:"
  Write-Host "    .\Merge-Apps.ps1 -Action Add-PortableApp -AppSource $($drive)\Apps"
  Dismount-VHD -Path $AppsVhdx
}

function Merge-AppLayer {
  if (-not (Test-Path $AppsVhdx)) { throw "$AppsVhdx 不存在" }
  if (-not (Test-Path $BaseVhdx)) { throw "$BaseVhdx 不存在" }
  if (-not (Test-Admin)) { Write-Error "Merge-VHD 需要管理员权限"; return }
  if (Get-VHD -Path $AppsVhdx -ErrorAction SilentlyContinue | Where-Object { $_.Attached }) {
    Write-Error "Apps.vhdx 仍挂载中, 请先 Dismount-VHD"
    return
  }
  Write-Host ">>> 备份当前 Base -> Base.pre-merge.vhdx" -ForegroundColor Yellow
  if (-not $DryRun -and -not (Test-Path "$BaseVhdx.pre-merge")) {
    Copy-Item $BaseVhdx "$BaseVhdx.pre-merge" -Force
  }
  Write-Host ">>> 将 Apps 层合并回 Base (Merge-VHD)" -ForegroundColor Cyan
  if ($DryRun) {
    Write-Host "    [DryRun] Merge-VHD -Path $AppsVhdx -DestinationPath $BaseVhdx"
  } else {
    Merge-VHD -Path $AppsVhdx -DestinationPath $BaseVhdx
  }
  Write-Host ">>> 优化 Base (Optimize-VHD -Mode Full)" -ForegroundColor Cyan
  if ($DryRun) {
    Write-Host "    [DryRun] Optimize-VHD -Path $BaseVhdx -Mode Full"
  } else {
    Optimize-VHD -Path $BaseVhdx -Mode Full
  }
  Write-Host ">>> 合并完成。新的 Base 已包含 Apps 层内容, Apps.vhdx 可删除重建" -ForegroundColor Green
  Write-Host "    删除旧 Apps 层: Remove-Item $AppsVhdx ; 再运行 -Action Create 创建新层" -ForegroundColor Yellow
}

function Add-PortableApp {
  if (-not $AppName) { throw "-AppName 必填" }
  if (-not $AppSource) { throw "-AppSource 必填 (绿色软件解压目录)" }
  if (-not (Test-Path $AppSource)) { throw "来源目录不存在: $AppSource" }
  $targetRoot = Join-Path $DataRoot "Apps"
  $target = Join-Path $targetRoot $AppName
  New-Item -ItemType Directory -Force -Path $targetRoot | Out-Null
  if (Test-Path $target) {
    if (-not $Force) { throw "目标已存在: $target ; 使用 -Force 覆盖" }
    Remove-Item $target -Recurse -Force
  }
  Write-Host ">>> 复制 $AppSource -> $target (绿色软件, 实体留在 Data)" -ForegroundColor Cyan
  robocopy $AppSource $target /E /R:2 /W:2 /MT:8 /XJ | Out-Null
  Write-Host ">>> 创建 C 盘符号链接 (mklink /J)" -ForegroundColor Cyan
  if ($DryRun) {
    Write-Host "    [DryRun] New-Item -ItemType Junction -Path C:\Program Files\$AppName -Target $target"
  } else {
    $linkPath = "C:\Program Files\$AppName"
    New-Item -ItemType Directory -Force -Path "C:\Program Files" | Out-Null
    if (Test-Path $linkPath) {
      if (-not $Force) { throw "链接已存在: $linkPath ; 使用 -Force 覆盖" }
      Remove-Item $linkPath -Recurse -Force
    }
    New-Item -ItemType Junction -Path $linkPath -Target $target | Out-Null
  }
  Write-Host ">>> 完成: $AppName 已装入 Data\Apps, C 盘仅符号链接" -ForegroundColor Green
  Write-Host "    验证: .\Merge-Apps.ps1 -Action Status"    -ForegroundColor Yellow
}

function Show-LayerStatus {
  Write-Host "===== 层式 VHDX 状态 =====" -ForegroundColor Cyan
  $tables = @(
    @{ Name = "Base (只读母盘)"; Path = $BaseVhdx },
    @{ Name = "Apps (可分发层)"; Path = $AppsVhdx },
    @{ Name = "User (动态差分)"; Path = $UserVhdx }
  )
  foreach ($t in $tables) {
    if (Test-Path $t.Path) {
      $v = Get-VHD -Path $t.Path -ErrorAction SilentlyContinue
      if ($v) {
        "{0,-22} {1}  Size={2:N1}GB  Used={3:N1}GB  Type={4}" -f $t.Name, $t.Path, ($v.Size/1GB), ($v.FileSize/1GB), $v.VhdType
      } else {
        "{0,-22} {1}  存在但无法读取(可能被占用)" -f $t.Name, $t.Path
      }
    } else {
      "{0,-22} {1}  <缺失>" -f $t.Name, $t.Path
    }
  }
  Write-Host "===== Data 符号链接 =====" -ForegroundColor Cyan
  if (Test-Path $DataRoot) {
    Get-ChildItem $DataRoot -Force | ForEach-Object {
      $sym = Test-StorageLayer $_.FullName
      "$($_.FullName)   $(if($sym){'[链接]'}else{'[实体]'})"
    }
  } else {
    "DataRoot 不存在: $DataRoot"
  }
  if (Test-Path "C:\Program Files") {
    Get-ChildItem "C:\Program Files" -Force | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint } | ForEach-Object {
      "$($_.FullName) -> $($_.Target)"
    }
  }
}

switch ($Action) {
  "Create"        { New-AppLayer }
  "Merge"         { Merge-AppLayer }
  "Add-PortableApp" { Add-PortableApp }
  "Status"        { Show-LayerStatus }
}
