#!/usr/bin/env python3
"""BCD 加 VARIX 引导项提权驱动：跑 bcd-addvarix.ps1，轮询日志至 BCD DONE / BCD ABORT。"""
import ctypes
import os
import sys
import time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
SCRIPT = ROOT + r"\_attic\bcd-addvarix.ps1"
LOG = ROOT + r"\_attic\bcd-addvarix.log"

# try/catch 兜终止异常；语句级 *> 落日志（PS5.1 默认 UTF-16LE）
params = (
    '-NoProfile -ExecutionPolicy Bypass -Command '
    f'try {{ & \'{SCRIPT}\' }} catch {{ $_ | Out-String }} *> \'{LOG}\''
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


print("BCD 引导项写入已启动（UAC 弹窗点『是』），轮询…")
seen = ""
deadline = time.time() + 180
while time.time() < deadline:
    time.sleep(3)
    tail = read_log() if os.path.exists(LOG) else ""
    new = tail[len(seen):]
    if new.strip():
        print(new, end="", flush=True)
        seen = tail
    if "BCD DONE" in tail or "BCD ABORT" in tail:
        break
print("\n[驱动] BCD 轮询结束")
