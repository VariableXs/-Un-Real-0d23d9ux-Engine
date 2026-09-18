#!/usr/bin/env python3
"""部署产物提权核验驱动：五分区卷重新挂盘符 -> 读取 ESP/VARIX_SYS/SHARED 内容落日志。"""
import ctypes
import os
import sys
import time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
PS1 = ROOT + r"\_attic\verify-prod.ps1"
LOG = ROOT + r"\_attic\verify-prod.log"

params = (
    '-NoProfile -ExecutionPolicy Bypass -Command '
    f'try {{ & \'{PS1}\' *> \'{LOG}\' }} '
    f'catch {{ $_ | Out-String | Add-Content \'{LOG}\' }}'
)

if os.path.exists(LOG):
    os.remove(LOG)

rc = ctypes.windll.shell32.ShellExecuteW(
    None, "runas", "powershell.exe", params, None, 0)  # SW_HIDE=0
if rc <= 32:
    print(f"UAC 提权被取消或失败 (ShellExecuteW rc={rc})")
    sys.exit(2)

print("提权核验已启动（UAC 弹窗点『是』），轮询…")
seen = ""
deadline = time.time() + 300
while time.time() < deadline:
    time.sleep(4)
    if not os.path.exists(LOG):
        continue
    with open(LOG, "rb") as f:
        raw = f.read()
    tail = raw.decode("utf-16", "replace") if raw.startswith(b"\xff\xfe") else raw.decode("gbk", "replace")
    new = tail[len(seen):]
    if new.strip():
        print(new, end="", flush=True)
        seen = tail
    if "VERIFY DONE" in tail:
        break
print("\n[驱动] 核验轮询结束")
