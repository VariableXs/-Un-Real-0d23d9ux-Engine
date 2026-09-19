#!/usr/bin/env python3
"""实证 L: 的真实设备归属：QueryDosDevice + 分区交叉验证。"""
import ctypes
import subprocess

buf = ctypes.create_unicode_buffer(1024)
n = ctypes.windll.kernel32.QueryDosDeviceW("L:", buf, 1024)
if n:
    print(f"L: -> {buf.value}")
else:
    print(f"L: QueryDosDevice failed err={ctypes.GetLastError()}")

r = subprocess.run(
    ["powershell", "-NoProfile", "-Command",
     "Get-Partition | Where-Object {$_.DriveLetter -eq 'L'} | "
     "Select-Object DiskNumber,PartitionNumber,DriveLetter,GptType,@{n='SizeGB';e={[math]::Round($_.Size/1GB,2)}} | "
     "Format-List | Out-String"],
    capture_output=True, timeout=60,
)
print(r.stdout.decode("gbk", "replace"))
