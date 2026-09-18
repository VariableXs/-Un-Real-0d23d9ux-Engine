# UEFI 隔离测试磁盘构建 v2：全 PowerShell Storage 命令（不再解析 diskpart 文本）
# diskpart 只负责 create（Storage 模块无建 VHD 能力），挂载/分区/格式化/拷贝/卸载全走 cmdlet
$ErrorActionPreference = 'Continue'
$ROOT = 'D:\2\14\-Un-Real-0d23d9ux-Engine-main'
$VHD = "$ROOT\_attic\uefi-esp.vhd"
$ISO = "$ROOT\build\isoroot"

Write-Output "=== cleanup leftover mount (run1) ==="
try { Dismount-DiskImage -ImagePath $VHD -ErrorAction Stop | Out-Null; Write-Output "leftover detached" }
catch { Write-Output ("no leftover: " + $_.Exception.Message) }
if (Test-Path $VHD) { Remove-Item $VHD -Force; Write-Output "old vhd removed" }

Write-Output "=== create vhd (diskpart create only) ==="
$f = Join-Path $env:TEMP 'vhd-create-dp.txt'
[IO.File]::WriteAllText($f, "create vdisk file=`"$VHD`" maximum=64 type=fixed`r`n", [Text.Encoding]::ASCII)
diskpart /s $f 2>&1 | Out-String | Write-Output
if (-not (Test-Path $VHD)) { Write-Output "!! vhd 未创建"; Write-Output "VHD DONE"; exit }

Write-Output "=== mount + locate disk ==="
Mount-DiskImage -ImagePath $VHD -ErrorAction Stop | Out-Null
$d = Get-DiskImage -ImagePath $VHD | Get-Disk
Write-Output ("vhd disk number: " + $d.Number + " size: " + $d.Size)
if ($d.Size -ne 64MB) { Write-Output "!! 盘大小不符"; Write-Output "VHD DONE"; exit }

Write-Output "=== GPT + ESP partition + FAT32 ==="
Initialize-Disk $d.Number -PartitionStyle GPT
$part = New-Partition -DiskNumber $d.Number -GptType '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' -UseMaximumSize -AssignDriveLetter
$vol = $part | Get-Volume
Format-Volume -DriveLetter $vol.DriveLetter -FileSystem FAT32 -NewFileSystemLabel 'VARIX-ESP' | Out-Null
$letter = (Get-Partition -DiskNumber $d.Number | Select-Object -First 1).DriveLetter
Write-Output ("esp letter: " + $letter)
if (-not $letter) { Write-Output "!! 无盘符"; Write-Output "VHD DONE"; exit }
$lp = $letter + ':\'
if ((Get-Volume -DriveLetter $letter).FileSystemLabel -ne 'VARIX-ESP') {
  Write-Output "!! 标签不符，落点不对"; Write-Output "VHD DONE"; exit
}

Write-Output "=== copy five files ==="
New-Item -ItemType Directory -Path ($lp + 'EFI\BOOT') -Force | Out-Null
New-Item -ItemType Directory -Path ($lp + 'kernel') -Force | Out-Null
Copy-Item "$ISO\EFI\BOOT\BOOTX64.EFI" ($lp + 'EFI\BOOT\BOOTX64.EFI') -Force
Copy-Item "$ISO\limine-bios.sys" ($lp + 'limine-bios.sys') -Force
Copy-Item "$ISO\limine.conf" ($lp + 'limine.conf') -Force
Copy-Item "$ISO\kernel\varix" ($lp + 'kernel\varix') -Force
Copy-Item "$ISO\initrd.img" ($lp + 'initrd.img') -Force

Write-Output "=== hashes on vhd ==="
foreach ($p in 'EFI\BOOT\BOOTX64.EFI','limine-bios.sys','limine.conf','kernel\varix','initrd.img') {
  $h = (Get-FileHash ($lp + $p) -Algorithm SHA256).Hash
  Write-Output ($p + " " + $h.Substring(0,16))
}

Write-Output "=== detach ==="
Dismount-DiskImage -ImagePath $VHD | Out-Null
Write-Output ("vhd size: " + (Get-Item $VHD).Length)
Write-Output "VHD DONE"
