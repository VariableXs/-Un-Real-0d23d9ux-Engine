param(
  [Parameter(Mandatory=$true)][string]$IsoPath,
  [string]$OutDir = "D:\Variable-USB",
  [int]$SizeGB = 150,
  [switch]$Fixed
)
# Variable OS - 2TB高速双接口优化版
# 用法: .\Create-VHDX.ps1 -IsoPath C:\Win11_22H2.iso -OutDir D:\Variable-USB -SizeGB 150
# 机械盘加 -Fixed，固态默认动态
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  Write-Error "请以管理员运行 PowerShell"; exit 1
}
$vhdx = Join-Path $OutDir "Variable-OS.vhdx"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
Write-Host ">>> 创建 VHDX $vhdx $SizeGB GB $(if($Fixed){'固定'}else{'动态'})" -ForegroundColor Cyan
if (Test-Path $vhdx) { Write-Host "已存在，将复用" -ForegroundColor Yellow } else {
  if ($Fixed) { New-VHD -Path $vhdx -SizeBytes ($SizeGB*1GB) -Fixed | Out-Null }
  else { New-VHD -Path $vhdx -SizeBytes ($SizeGB*1GB) -Dynamic | Out-Null }
}
Write-Host ">>> 挂载 VHDX" -ForegroundColor Cyan
Mount-VHD -Path $vhdx -PassThru | Out-Null
$disk = Get-Disk | Where-Object {$_.Location -like "*$([IO.Path]::GetFileName($vhdx))*"} | Select-Object -First 1
Initialize-Disk -Number $disk.Number -PartitionStyle GPT -PassThru | Out-Null
$part = New-Partition -DiskNumber $disk.Number -UseMaximumSize -AssignDriveLetter
Format-Volume -Partition $part -FileSystem NTFS -NewFileSystemLabel "VariableOS" -Confirm:$false -AllocationUnitSize 64KB | Out-Null
$drive = (Get-Partition -DiskNumber $disk.Number | Where-Object { $_.DriveLetter }).DriveLetter + ":"
Write-Host "已格式化 $drive (64KB簇)" -ForegroundColor Green
Write-Host ">>> 挂载 ISO $IsoPath" -ForegroundColor Cyan
$isoMount = Mount-DiskImage -ImagePath $IsoPath -PassThru | Get-Volume
$isoLetter = $isoMount.DriveLetter + ":"
$wim = Get-ChildItem -Path "$isoLetter\sources" -Filter "*.wim" | Select-Object -First 1
if (-not $wim) { $wim = Get-ChildItem -Path "$isoLetter\sources" -Filter "*.esd" | Select-Object -First 1 }
if (-not $wim) { throw "未找到 install.wim/esd" }
Write-Host ">>> 解压镜像 $($wim.FullName) -> $drive (约10分钟)" -ForegroundColor Cyan
dism /Apply-Image /ImageFile:$($wim.FullName) /Index:6 /ApplyDir:$drive\
Write-Host ">>> 写入引导" -ForegroundColor Cyan
bcdboot "$drive\Windows" /s $drive /f UEFI
# 优化
Write-Host ">>> 优化 CompactOS" -ForegroundColor Cyan
dism /Image:$drive\ /Compact:On 2>$null | Out-Null
Dismount-DiskImage -ImagePath $IsoPath | Out-Null
Dismount-VHD -Path $vhdx
Write-Host ">>> 完成 $vhdx 可直接用 Test-VM.ps1 测试" -ForegroundColor Green
Write-Host "下一步: .\Test-VM.ps1 -Vhdx $vhdx"
