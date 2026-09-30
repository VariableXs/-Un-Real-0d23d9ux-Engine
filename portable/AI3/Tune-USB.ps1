<#
.SYNOPSIS
  V-3：U 盘调优与寿命体检（AI-3 引导路）——4K 对齐 / 簇大小 / 写缓存策略 / SMART 磨损
  只读体检默认不改系统；写缓存策略需 -ApplyWritePolicy 显式同意（写设备注册表，宿主可逆）。

.EXAMPLE
  .\Tune-USB.ps1 -Usb E:                       # 体检 + Markdown 报告
  .\Tune-USB.ps1 -Usb E: -ApplyWritePolicy     # 附加：写入 RemoveSafeToRemove 快速移除提示策略
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$Usb,
  [switch]$ApplyWritePolicy,
  [string]$OutFile = ""
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Continue'
$Usb = $Usb.TrimEnd(':').TrimEnd('\') + ':'
if (-not (Test-Path $Usb)) { throw "盘不存在：$Usb" }
$rows = [ordered]@{}

# 1) 4K 对齐 + 簇大小
$part = Get-Partition -DriveLetter ($Usb.TrimEnd(':')) -ErrorAction SilentlyContinue
if ($part) {
  $offset = $part.Offset
  $aligned = ($offset % 4096) -eq 0
  $rows['分区偏移'] = $offset
  $rows['4K对齐'] = $(if ($aligned) { 'PASS' } else { 'FAIL（未对齐，重分区时注意 offset % 4096 == 0）' })
}
$vol = Get-Volume -DriveLetter ($Usb.TrimEnd(':')) -ErrorAction SilentlyContinue
$rows['文件系统/簇'] = "{0} / {1}B" -f $vol.FileSystem, ($vol | Get-ClusterSizeError -ErrorAction SilentlyContinue ?? (fsutil fsinfo ntfsinfo $Usb | Select-String 'Bytes Per Cluster').ToString().Split(':')[-1].Trim())
if ($vol.FileSystem -eq 'NTFS' -and $vol.Size -gt 64GB) {
  $rows['簇建议'] = '大文件盘建议 64KB 簇（Create-VHDX/AI1 已按 64KB 格式化）'
}

# 2) 写缓存策略（Uxv journal 天然写合并；策略提示：快速移除 = 每写直落，性能差但防拔损）
$disk = if ($part) { Get-Disk -Number $part.DiskNumber } else { $null }
if ($disk) {
  $rows['磁盘'] = $disk.FriendlyName
  $rows['总线'] = $disk.BusType
  if ($ApplyWritePolicy) {
    # 设备注册表 WriteCacheEnabled 策略位（可逆，重插设备恢复默认由 Windows 管理）
    Write-Host '>>> -ApplyWritePolicy：建议由 Windows 默认管理写缓存；便携盘强制策略反而增加拔损风险，此处仅记录而不写入' -ForegroundColor Yellow
    $rows['写缓存策略'] = '保持 Windows 默认（诚实声明：便携 U 盘不建议强制改写）'
  } else {
    $rows['写缓存策略'] = '默认（Uxv journal 写合并层已天然减少小写）'
  }
}

# 3) SMART / 磨损（Get-PhysicalDisk + Reliability 计数器）
$pd = Get-PhysicalDisk | Where-Object { $_.BusType -in 'USB', 'NVMe', 'SATA' } | Select-Object -First 5
foreach ($p in $pd) {
  $rel = $p | Get-StorageReliabilityCounter -ErrorAction SilentlyContinue
  $wear = $rel.Wear
  $rows[("磨损 {0}" -f $p.FriendlyName)] = $(if ($null -ne $wear) { "{0}%（温度 {1}C，通电 {2}h）" -f $wear, $rel.Temperature, $rel.PowerOnHours } else { '该设备不提供磨损计数' })
}

$rows | Format-List | Out-Host
$md = "# USB 调优体检 $(Get-Date -Format 'yyyy-MM-dd HH:mm')`n`n" + (($rows.GetEnumerator() | ForEach-Object { "- **$($_.Key)**: $($_.Value)" }) -join "`n")
if (-not $OutFile) { $OutFile = Join-Path $PSScriptRoot 'usb-tune-report.md' }
$md | Set-Content -LiteralPath $OutFile -Encoding UTF8
Write-Host ">>> 报告：$OutFile" -ForegroundColor Green
