#!/usr/bin/env python3
"""ESP 只读 dump 驱动：BOM→语法预检→提权→轮询（kern-update 范式）。"""
import ctypes
import os
import subprocess
import sys
import time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
SCRIPT = ROOT + r"\_attic\esp-dump-ro.ps1"
LOG = ROOT + r"\_attic\esp-dump-ro.log"
CHECK = ROOT + r"\_attic\bcd-fix2-parse-check.py"

raw = open(SCRIPT, "rb").read()
if not raw.startswith(b"\xef\xbb\xbf"):
    open(SCRIPT, "wb").write(b"\xef\xbb\xbf" + raw)
    print("BOM added")

r = subprocess.run([sys.executable, CHECK, SCRIPT], capture_output=True)
out = r.stdout.decode("gbk", "replace") + r.stderr.decode("gbk", "replace")
if "ParseErrors: 0" not in out:
    print(out)
    raise SystemExit("!! ps1 语法预检未过")

if os.path.exists(LOG):
    os.remove(LOG)

params = (
    '-NoProfile -ExecutionPolicy Bypass -Command '
    f'try {{ & \'{SCRIPT}\' *> \'{LOG}\' }} '
    f'catch {{ $_ | Out-String | Add-Content \'{LOG}\' }}'
)
rc = ctypes.windll.shell32.ShellExecuteW(None, "runas", "powershell.exe", params, None, 0)
if rc <= 32:
    print(f"UAC 被取消 (rc={rc})")
    sys.exit(2)

print("只读 dump 已启动（UAC 点『是』，不会写盘），轮询…")
deadline = time.time() + 90
final = ""
while time.time() < deadline:
    time.sleep(2)
    if os.path.exists(LOG):
        log = open(LOG, "rb").read().decode("utf-16", "replace")
        if "ESP-DUMP-DONE" in log:
            final = log
            break
print(final if final else "超时——日志不存在")
