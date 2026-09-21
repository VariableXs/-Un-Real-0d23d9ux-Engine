# -*- coding: utf-8 -*-
"""SHARED 卷失踪核查：全卷列表 + 磁盘列表（只读）。"""
import subprocess
import sys

PS = r"""
$ErrorActionPreference='SilentlyContinue'
Get-Volume | Where-Object { $_.DriveLetter } | ForEach-Object {
  Write-Output ('VOL|' + $_.DriveLetter + '|' + $_.FileSystemLabel + '|' + $_.FileSystem + '|' + [math]::Round($_.Size/1GB,1) + 'GB')
}
Get-Disk | ForEach-Object {
  Write-Output ('DISK|' + $_.Number + '|' + $_.BusType + '|' + $_.FriendlyName + '|' + [math]::Round($_.Size/1GB,1) + 'GB')
}
"""
out = subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command", PS],
                     capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=60)
sys.stdout.write(out.stdout)
if out.stderr:
    sys.stderr.write(out.stderr[-200:])
