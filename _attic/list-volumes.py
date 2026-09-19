#!/usr/bin/env python3
"""列出所有带标签的卷(U 盘定位)。"""
import subprocess

r = subprocess.run(
    ["powershell", "-NoProfile", "-Command",
     "Get-Volume | Where-Object {$_.FileSystemLabel} | "
     "Select-Object DriveLetter,FileSystemLabel,FileSystem,@{n='FreeGB';e={[math]::Round($_.SizeRemaining/1GB,2)}} | "
     "Format-Table -AutoSize | Out-String"],
    capture_output=True, timeout=60,
)
print(r.stdout.decode("gbk", "replace"))
print(r.stderr.decode("gbk", "replace"))
