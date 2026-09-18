#!/usr/bin/env python3
"""部署核验 v2 提权驱动：ShellExecuteW 提权跑 verify2-prod.ps1，轮询日志至 VERIFY2 DONE。"""
import ctypes
import os
import sys
import time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
SCRIPT = ROOT + r"\_attic\verify2-prod.ps1"
LOG = ROOT + r"\_attic\verify2-prod.log"

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
    """PS5.1 的 *> 重定向默认写 UTF-16LE（带 BOM），须按 BOM 自适应解码。"""
    with open(LOG, "rb") as f:
        raw = f.read()
    if raw.startswith(b"\xff\xfe"):
        return raw.decode("utf-16", "replace")
    return raw.decode("gbk", "replace")


print("提权核验 v2 已启动（UAC 弹窗点『是』），轮询…")
seen = ""
deadline = time.time() + 300
while time.time() < deadline:
    time.sleep(3)
    tail = read_log() if os.path.exists(LOG) else ""
    new = tail[len(seen):]
    if new.strip():
        print(new, end="", flush=True)
        seen = tail
    if "VERIFY2 DONE" in tail:
        break
print("\n[驱动] 核验轮询结束")
