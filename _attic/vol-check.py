#!/usr/bin/env python3
"""实证：查五卷盘符现状 + VARIX-ESP 卷的 DriveLetter 字段值。"""
import subprocess

PS = r"""
Get-Volume | Where-Object { $_.DriveLetter } | Sort-Object DriveLetter |
  Select-Object DriveLetter,FileSystemLabel,@{n='GB';e={[math]::Round($_.Size/1GB,1)}} |
  Format-Table -AutoSize | Out-String -Width 200
'--- VARIX-ESP volume detail ---'
Get-Volume -FileSystemLabel 'VARIX-ESP' -ErrorAction SilentlyContinue |
  Select-Object DriveLetter,ObjectId | Format-List | Out-String
'--- SHARED volume detail ---'
Get-Volume -FileSystemLabel 'SHARED' -ErrorAction SilentlyContinue |
  Select-Object DriveLetter,ObjectId | Format-List | Out-String
"""

r = subprocess.run(["powershell.exe", "-NoProfile", "-Command", PS], capture_output=True)
print(r.stdout.decode("gbk", "replace"))
err = r.stderr.decode("gbk", "replace")
if err.strip():
    print("[stderr]", err)
