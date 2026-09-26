#!/usr/bin/env python3
"""只读核验：U 盘 ESP limine.conf 与 repo 根逐字节一致（FNV-1a 正文 hash 对表）。
不写盘、不删盘符以外的任何东西；结束即卸载盘符。"""
import ctypes, hashlib, os, subprocess, sys, time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
SCRIPT = ROOT + r"\_attic\esp-verify-conf.ps1"
LOG = ROOT + r"\_attic\esp-verify-conf.log"

raw = open(SCRIPT, "rb").read()
if not raw.startswith(b"\xef\xbb\xbf"):
    open(SCRIPT, "wb").write(b"\xef\xbb\xbf" + raw)

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

print("核验已启动（UAC 点『是』），轮询…")
deadline = time.time() + 90
final = ""
while time.time() < deadline:
    time.sleep(2)
    if os.path.exists(LOG):
        log = open(LOG, "rb").read().decode("utf-16", "replace")
        if "VERIFY-CONF-DONE" in log or "VERIFY-CONF-FAIL" in log:
            final = log
            break
print(final if final else "超时——见日志")
