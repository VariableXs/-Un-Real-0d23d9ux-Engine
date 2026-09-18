#!/usr/bin/env python3
"""提权清理 L: 盘符残留（diskpart remove）。"""
import ctypes
import os
import subprocess
import time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
SCRIPT = ROOT + r"\_attic\_rm-L.ps1"
LOG = ROOT + r"\_attic\_rm-L.log"

with open(SCRIPT, "w", encoding="ascii", newline="\r\n") as f:
    f.write(
        "$ErrorActionPreference='Continue'\n"
        "select-string -Path 'x' -ErrorAction SilentlyContinue | Out-Null\n"
        "$dp = Join-Path $env:TEMP 'rm-L.txt'\n"
        "[IO.File]::WriteAllText($dp, \"select disk 1`r`nselect partition 1`r`nremove letter=L`r`n\", [Text.Encoding]::ASCII)\n"
        "diskpart /s $dp | Out-String | Write-Output\n"
        "Write-Output ('L-Test-Path: ' + (Test-Path 'L:\\'))\n"
        "Write-Output 'RM-L-DONE'\n"
    )

if os.path.exists(LOG):
    os.remove(LOG)
params = (
    '-NoProfile -ExecutionPolicy Bypass -Command '
    f'try {{ & \'{SCRIPT}\' *> \'{LOG}\' }} '
    f'catch {{ $_ | Out-String | Add-Content \'{LOG}\' }}'
)
rc = ctypes.windll.shell32.ShellExecuteW(None, "runas", "powershell.exe", params, None, 0)
if rc <= 32:
    raise SystemExit(f"UAC 被取消 (rc={rc})")
deadline = time.time() + 60
while time.time() < deadline:
    time.sleep(2)
    if os.path.exists(LOG):
        log = open(LOG, "rb").read().decode("utf-16", "replace")
        if "RM-L-DONE" in log:
            print(log)
            break
else:
    print("超时")
