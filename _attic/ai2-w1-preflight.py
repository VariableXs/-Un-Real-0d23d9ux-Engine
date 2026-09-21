# -*- coding: utf-8 -*-
"""AI-2 · W1 部署预检（严格只读）。

三重确认闸门的落点实证（部署手册 §2）：目标盘必须是 U 盘的 WIN_ENGINE 分区，
内置盘绝不出现在候选列表。任何一项不过 → PREFLIGHT=FAIL → 退出不动。

用法：python _attic/ai2-w1-preflight.py
（控制台中文可能按 GBK 显示为乱码，判定以行内 PASS/FAIL 布尔为准。）
"""
import subprocess
import sys

PS = r"""
$ErrorActionPreference = 'SilentlyContinue'
$fail = 0
# 候选：WIN_ENGINE 卷（Get-Volume 的 FreeSpace 在部分环境恒 0，容量/可用走 Win32_LogicalDisk 权威口径）
$v = Get-Volume -FileSystemLabel 'WIN_ENGINE' | Select-Object -First 1
if (-not $v) { Write-Output 'PREFLIGHT=FAIL'; Write-Output 'REASON|WIN_ENGINE volume not found'; exit 0 }
$dl = $v.DriveLetter
Write-Output ("CHECK|label=WIN_ENGINE|PASS")
Write-Output ("CHECK|drive=" + $dl + "|INFO")
$ld = Get-CimInstance Win32_LogicalDisk -Filter "DeviceID='$($dl):'"
$sizeGB = [math]::Round($ld.Size/1GB, 1)
$freeGB = [math]::Round($ld.FreeSpace/1GB, 1)
$usedGB = [math]::Round($sizeGB - $freeGB, 2)
Write-Output ("CHECK|sizeGB=" + $sizeGB + "|INFO")
Write-Output ("CHECK|freeGB=" + $freeGB + "|INFO")
if ([math]::Abs($sizeGB - 300) -gt 30) { Write-Output 'CHECK|size~300GB|FAIL'; $fail++ } else { Write-Output 'CHECK|size~300GB|PASS' }
Write-Output ("CHECK|fs=" + $v.FileSystem + "|INFO")
if ($v.FileSystem -ne 'NTFS') { Write-Output 'CHECK|fs=NTFS|FAIL'; $fail++ } else { Write-Output 'CHECK|fs=NTFS|PASS' }
if ($usedGB -gt 5) { Write-Output ("CHECK|usedGB=" + $usedGB + "|FAIL"); $fail++ } else { Write-Output ("CHECK|usedGB=" + $usedGB + "|PASS") }
# 物理盘对照：USB 盘必须在场（内置盘 NVMe 不得出现在候选语义里）
$usbDisks = @(Get-Disk | Where-Object { $_.BusType -match 'USB' })
$usbDisks | ForEach-Object { Write-Output ("DISK|" + $_.Number + "|" + $_.BusType + "|" + $_.FriendlyName + "|" + [math]::Round($_.Size/1GB,1) + "GB") }
if ($usbDisks.Count -eq 0) { Write-Output 'CHECK|usb-present|FAIL'; $fail++ } else { Write-Output ('CHECK|usb-present=' + $usbDisks.Count + '|PASS') }
# 内置盘 ESP 基线对照（只读；GUID 归一化去花括号后比对）
$esp = Get-Partition | Where-Object { $_.GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' -and $_.DiskNumber -eq 0 } | Select-Object -First 1
if ($esp) {
  $g = ("$($esp.Guid)" -replace '[{}]', '').ToLower()
  Write-Output ("CHECK|internal-esp-guid=" + $g + "|INFO")
  if ($g -ne '425214ee-257c-4aec-b69c-40178a4e847b') { Write-Output 'CHECK|internal-esp-vs-baseline|FAIL'; $fail++ }
  else { Write-Output 'CHECK|internal-esp-vs-baseline|PASS' }
} else {
  Write-Output 'CHECK|internal-esp-visible|SKIP'
}
if ($fail -gt 0) { Write-Output ("PREFLIGHT=FAIL(" + $fail + ")") } else { Write-Output 'PREFLIGHT=PASS' }
"""

out = subprocess.run(
    ["powershell", "-NoProfile", "-NonInteractive", "-Command", PS],
    capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=120,
)
sys.stdout.write(out.stdout)
if out.returncode != 0:
    sys.stderr.write(out.stderr)
    sys.exit(out.returncode)
