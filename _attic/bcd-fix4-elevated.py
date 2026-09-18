#!/usr/bin/env python3
"""BCD fix4 提权驱动：跑 bcd-fix4.ps1（先补 BOM），轮询至 BCD4 DONE。"""
import ctypes
import os
import sys
import time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
SCRIPT = ROOT + r"\_attic\bcd-fix4.ps1"
LOG = ROOT + r"\_attic\bcd-fix4.log"

# 补 BOM（PS5.1 无 BOM 按 GBK 读，中文字符串乱码炸引号）
raw = open(SCRIPT, "rb").read()
if not raw.startswith(b"\xef\xbb\xbf"):
    open(SCRIPT, "wb").write(b"\xef\xbb\xbf" + raw)
    print("BOM added to bcd-fix4.ps1")

# *> 必须在 try 块内部包住 & 调用（try/catch 语句外跟 *> 是 PS5.1 非法语法）
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


print("修复 v4 已启动（UAC 弹窗点『是』），轮询…")
seen = ""
deadline = time.time() + 180
while time.time() < deadline:
    time.sleep(3)
    tail = read_log() if os.path.exists(LOG) else ""
    new = tail[len(seen):]
    if new.strip():
        print(new, end="", flush=True)
        seen = tail
    if "BCD4 DONE" in tail:
        break
print("\n[驱动] 轮询结束")
