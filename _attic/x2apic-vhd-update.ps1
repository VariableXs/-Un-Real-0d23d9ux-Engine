# x2APIC 测试用 VHD 更新：挂载 uefi-esp.vhd → 写测试 limine.conf + 新内核 → 卸载
$ErrorActionPreference = 'Stop'
$DISK = 'D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\uefi-esp.vhd'
$CONF = 'D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\x2apic-limine.conf'
$KERN = 'D:\2\14\-Un-Real-0d23d9ux-Engine-main\kernel\target\x86_64-unknown-none\release\varix'
try {
  $img = Mount-DiskImage -ImagePath $DISK -PassThru
  $letter = ($img | Get-Disk | Get-Partition | Get-Volume).DriveLetter
  Write-Output ("letter=[" + $letter + "]")
  if (-not $letter) { throw "no drive letter after mount" }
  Copy-Item $CONF "$($letter):\limine.conf" -Force
  Copy-Item $KERN "$($letter):\kernel\varix" -Force
  Write-Output "copied: limine.conf + kernel"
  Write-Output ("kern size=" + (Get-Item "$($letter):\kernel\varix").Length)
  Dismount-DiskImage -ImagePath $DISK | Out-Null
  Write-Output "VHD-UPDATE-DONE"
} catch {
  Write-Output ("UPDATE-FAIL: " + $_.Exception.Message)
  try { Dismount-DiskImage -ImagePath $DISK | Out-Null } catch { }
}
