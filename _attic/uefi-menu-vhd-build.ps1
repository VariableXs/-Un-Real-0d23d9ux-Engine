# UEFI 三卡菜单走查磁盘构建（专用，与正式 uefi-esp.vhd 互不干扰）
#
# 目的：做出 GPT + FAT32 ESP 的 VHD，让 OVMF 以**普通块设备**引导 Limine——
# 这是唯一能真验证 BootNext / OsIndications 的平台（ISO+OVMF 会撞 Limine 的
# "Could not meaningfully match the boot device handle" BUG，实测确认）。
#
# 戒律（沿用项目既有约定）：
#   * 卷访问按**标签**定位 + assign 后回读标签确认落点（VARIX-ESP-MENU）
#   * 落点内容实证不过就退出，不动盘
#   * 磁盘绝不误伤：只操作本脚本自己创建/挂载的 VHD
$ErrorActionPreference = 'Continue'
$ROOT = 'D:\2\14\-Un-Real-0d23d9ux-Engine-main'
$VHD = "$ROOT\_attic\uefi-menu.vhd"
$SRC = "$ROOT\_attic\uefi-menu-root"
$LABEL = 'VARIX-ESP-MENU'

Write-Output "=== 0. 前置检查 ==="
if (-not (Test-Path $SRC)) { Write-Output "!! 源目录不存在: $SRC"; Write-Output "VHD DONE"; exit }
foreach ($f in 'EFI\BOOT\BOOTX64.EFI','kernel\varix','limine.conf','limine-bios.sys','initrd.img') {
  if (-not (Test-Path (Join-Path $SRC $f))) { Write-Output "!! 源缺文件: $f"; Write-Output "VHD DONE"; exit }
}
Write-Output "源文件齐备"

Write-Output "=== 1. 清理旧挂载/旧盘 ==="
try { Dismount-DiskImage -ImagePath $VHD -ErrorAction Stop | Out-Null; Write-Output "旧挂载已摘除" }
catch { Write-Output "无旧挂载" }
if (Test-Path $VHD) { Remove-Item $VHD -Force; Write-Output "旧 VHD 已删" }

Write-Output "=== 2. 建 VHD（diskpart 仅负责 create） ==="
$dp = Join-Path $env:TEMP 'vhd-menu-create.txt'
[IO.File]::WriteAllText($dp, "create vdisk file=`"$VHD`" maximum=96 type=fixed`r`n", [Text.Encoding]::ASCII)
diskpart /s $dp 2>&1 | Out-String | Write-Output
if (-not (Test-Path $VHD)) { Write-Output "!! VHD 未创建"; Write-Output "VHD DONE"; exit }

Write-Output "=== 3. 挂载 + 定位 ==="
Mount-DiskImage -ImagePath $VHD -ErrorAction Stop | Out-Null
$d = Get-DiskImage -ImagePath $VHD | Get-Disk
Write-Output ("VHD disk #" + $d.Number + " size=" + $d.Size)
if ($d.Size -ne 96MB) { Write-Output "!! 盘大小不符，退出不动"; Dismount-DiskImage -ImagePath $VHD | Out-Null; Write-Output "VHD DONE"; exit }

Write-Output "=== 4. GPT + ESP + FAT32 ==="
Initialize-Disk $d.Number -PartitionStyle GPT | Out-Null
$part = New-Partition -DiskNumber $d.Number -GptType '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' -UseMaximumSize -AssignDriveLetter
$vol = $part | Get-Volume
Format-Volume -DriveLetter $vol.DriveLetter -FileSystem FAT32 -NewFileSystemLabel $LABEL -Confirm:$false | Out-Null
$letter = (Get-Partition -DiskNumber $d.Number | Select-Object -First 1).DriveLetter
if (-not $letter) { Write-Output "!! 无盘符"; Dismount-DiskImage -ImagePath $VHD | Out-Null; Write-Output "VHD DONE"; exit }

# 落点实证：按标签回读确认（戒律）
$back = (Get-Volume -DriveLetter $letter).FileSystemLabel
Write-Output ("盘符=" + $letter + " 回读标签=" + $back)
if ($back -ne $LABEL) { Write-Output "!! 标签不符，落点不对，退出不动"; Dismount-DiskImage -ImagePath $VHD | Out-Null; Write-Output "VHD DONE"; exit }
$lp = $letter + ':\'

Write-Output "=== 5. 拷贝文件 ==="
New-Item -ItemType Directory -Path ($lp + 'EFI\BOOT') -Force | Out-Null
New-Item -ItemType Directory -Path ($lp + 'kernel') -Force | Out-Null
foreach ($f in 'EFI\BOOT\BOOTX64.EFI','limine-bios.sys','limine.conf','kernel\varix','initrd.img') {
  Copy-Item (Join-Path $SRC $f) ($lp + $f) -Force
  Write-Output ("copied " + $f)
}

Write-Output "=== 6. 回读校验（双端 sha256 前 16 位） ==="
$bad = 0
foreach ($f in 'EFI\BOOT\BOOTX64.EFI','limine-bios.sys','limine.conf','kernel\varix','initrd.img') {
  $a = (Get-FileHash (Join-Path $SRC $f) -Algorithm SHA256).Hash.Substring(0,16)
  $b = (Get-FileHash ($lp + $f) -Algorithm SHA256).Hash.Substring(0,16)
  $ok = if ($a -eq $b) { 'OK' } else { $bad++; 'MISMATCH' }
  Write-Output ($f.PadRight(24) + " src=$a vhd=$b $ok")
}
if ($bad -gt 0) { Write-Output "!! 有 $bad 个文件校验失败" } else { Write-Output "全部对齐" }

Write-Output "=== 7. 卸盘 ==="
Dismount-DiskImage -ImagePath $VHD | Out-Null
Write-Output ("VHD size: " + (Get-Item $VHD).Length)
Write-Output "VHD DONE"
