# -*- coding: utf-8 -*-
"""部署中状态核查：X: 已用增长 + 进程表扫描（python 侧，只读）。"""
import os
import subprocess
import time

def x_used_gb():
    out = subprocess.run(
        ["powershell", "-NoProfile", "-NonInteractive", "-Command",
         "$d=Get-CimInstance Win32_LogicalDisk -Filter \"DeviceID='X:'\"; "
         "Write-Output ('USED_GB=' + [math]::Round(($d.Size-$d.FreeSpace)/1GB,2))"],
        capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=30)
    for l in out.stdout.splitlines():
        if l.startswith("USED_GB="):
            return float(l.split("=")[1])
    return -1

def procs():
    out = subprocess.run(["tasklist", "/FO", "CSV"], capture_output=True, text=True,
                         encoding="utf-8", errors="replace", timeout=30)
    names = {"dism.exe": 0, "powershell.exe": 0}
    for line in out.stdout.splitlines():
        for n in names:
            if '"' + n + '"' in line:
                names[n] += 1
    return names

a = x_used_gb()
n1 = procs()
time.sleep(12)
b = x_used_gb()
n2 = procs()
print(f"X: used {a} GB -> {b} GB (delta {round(b-a,2)} GB / 12s)")
print(f"procs before: {n1}  after: {n2}")
