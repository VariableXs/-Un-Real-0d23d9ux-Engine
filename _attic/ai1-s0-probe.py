#!/usr/bin/env python3
"""AI-1 只读开工盘点：USB 盘/卷标签/快速启动键现值/SecureBoot。绝不写任何东西。"""
import subprocess
import re
import sys

PS = r"""
$ErrorActionPreference = 'SilentlyContinue'
Write-Output '=== USB DISKS ==='
Get-Disk | Where-Object BusType -eq 'USB' | ForEach-Object {
  Write-Output ("disk={0} name={1} sizeGB={2} state={3}" -f $_.Number, $_.FriendlyName, [math]::Round($_.Size/1GB,1), $_.OperationalStatus)
}
Write-Output '=== LABELED VOLUMES ==='
Get-Volume | Where-Object { $_.FileSystemLabel } | ForEach-Object {
  Write-Output ("letter={0} label={1} sizeGB={2} fs={3}" -f $_.DriveLetter, $_.FileSystemLabel, [math]::Round($_.Size/1GB,1), $_.FileSystem)
}
Write-Output '=== HIBERBOOT ==='
$v = (Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power' -Name HiberbootEnabled).HiberbootEnabled
Write-Output ("HiberbootEnabled={0}" -f $v)
Write-Output '=== SECUREBOOT ==='
try { Write-Output ("SecureBoot={0}" -f (Confirm-SecureBootUEFI)) } catch { Write-Output ("SecureBoot=QUERY-FAIL {0}" -f $_.Exception.Message) }
"""

r = subprocess.run(["powershell.exe", "-NoProfile", "-Command", PS],
                   capture_output=True, timeout=60)
raw = r.stdout + r.stderr
for enc in ("gbk", "utf-8", "utf-16"):
    try:
        out = raw.decode(enc)
        break
    except (UnicodeDecodeError, UnicodeError):
        continue
else:
    out = raw.decode("utf-8", "replace")
sys.stdout.buffer.write(out.encode("utf-8", "replace"))
