#!/usr/bin/env python3
"""VARIX_SYS 终验提权驱动：verify3-prod.ps1（装入点方案），轮询至 VERIFY3 DONE。"""
import ctypes
import os
import sys
import time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
SCRIPT = ROOT + r"\_attic\verify3-prod.ps1"
LOG = ROOT + r"\_attic\verify3-prod.log"

params = (
    '-NoProfile -ExecutionPolicy Bypass -Command '
    f'try {{ & \'{SCRIPT}\' *> \'{LOG}\' }} '
    f'catch {{ $_ | Out-String | Add-Content \'{LOG}\' }}'
)

if os.path.exists(LOG):
    os.remove(LOG)

rc = ctypes.windll.shell32.ShellExecuteW(
    None, "runas", "powershell.exe", params, None, 0)  # SW_HIDE=0
if rc <= 32:
    print(f"UAC 提权被取消或失败 (ShellExecuteW rc={rc})")
    sys.exit(2)


def read_log() -> str:
    with open(LOG, "rb") as f:
        raw = f.read()
    if raw.startswith(b"\xff\xfe"):
        return raw.decode("utf-16", "replace")
    return raw.decode("gbk", "replace")


print("提权核验 v3 已启动（UAC 弹窗点『是』），轮询…")
seen = ""
deadline = time.time() + 180
while time.time() < deadline:
    time.sleep(3)
    tail = read_log() if os.path.exists(LOG) else ""
    new = tail[len(seen):]
    if new.strip():
        print(new, end="", flush=True)
        seen = tail
    if "VERIFY3 DONE" in tail:
        break
print("\n[驱动] 核验轮询结束")
