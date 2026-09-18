#!/usr/bin/env python3
"""U 盘 ESP 内核更新提权驱动：kern-update-usb.ps1（BOM→预检→提权→轮询）。"""
import ctypes
import os
import subprocess
import sys
import time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
SCRIPT = ROOT + r"\_attic\kern-update-usb.ps1"
LOG = ROOT + r"\_attic\kern-update-usb.log"
CHECK = ROOT + r"\_attic\bcd-fix2-parse-check.py"

raw = open(SCRIPT, "rb").read()
if not raw.startswith(b"\xef\xbb\xbf"):
    open(SCRIPT, "wb").write(b"\xef\xbb\xbf" + raw)
    print("BOM added")

r = subprocess.run([sys.executable, CHECK, SCRIPT], capture_output=True)
out = r.stdout.decode("gbk", "replace") + r.stderr.decode("gbk", "replace")
print(out)
if "ParseErrors: 0" not in out:
    print("[驱动] 语法预检未过，放弃提权")
    sys.exit(3)

if os.path.exists(LOG):
    os.remove(LOG)

params = (
    '-NoProfile -ExecutionPolicy Bypass -Command '
    f'try {{ & \'{SCRIPT}\' *> \'{LOG}\' }} '
    f'catch {{ $_ | Out-String | Add-Content \'{LOG}\' }}'
)
rc = ctypes.windll.shell32.ShellExecuteW(None, "runas", "powershell.exe", params, None, 0)
if rc <= 32:
    print(f"UAC 提权被取消或失败 (rc={rc})")
    sys.exit(2)

print("内核更新已启动（UAC 点『是』），轮询…")
seen = ""
final = ""
deadline = time.time() + 120
while time.time() < deadline:
    time.sleep(3)
    tail = open(LOG, "rb").read().decode("utf-16", "replace") if os.path.exists(LOG) else ""
    new = tail[len(seen):]
    if new.strip():
        print(new, end="", flush=True)
        seen = tail
    if "KERN-UPDATE-DONE" in tail or "UPDATE-FAIL" in tail:
        final = tail
        break
print("\n[驱动] 轮询结束")
if "KERN-UPDATE-DONE" in final:
    print("[结果] U 盘 ESP 内核已更新为 x2APIC 修复版")
else:
    print("[结果] 更新未完成，见上方日志")
