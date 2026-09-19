#!/usr/bin/env python3
"""列出物理磁盘(找 U 盘 Disk 编号)。"""
import subprocess

r = subprocess.run(
    ["powershell", "-NoProfile", "-Command",
     "Get-Disk | Select-Object Number,FriendlyName,BusType,@{n='SizeGB';e={[math]::Round($_.Size/1GB,1)}} | "
     "Format-Table -AutoSize | Out-String"],
    capture_output=True, timeout=60,
)
print(r.stdout.decode("gbk", "replace"))
if r.stderr:
    print("ERR:", r.stderr.decode("gbk", "replace"))
