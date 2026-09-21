#!/usr/bin/env python3
"""S1.3 只读核验驱动：BOM→占位符注入→提权执行→轮询结果。不写任何业务数据。"""
import ctypes
import os
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "esp-win-boot-verify.ps1")
LOG = os.path.join(HERE, "esp-win-boot-verify.log")

raw = open(SCRIPT, "rb").read()
if not raw.startswith(b"\xef\xbb\xbf"):
    open(SCRIPT, "wb").write(b"\xef\xbb\xbf" + raw)

body = open(SCRIPT, "rb").read().decode("utf-8-sig").replace("{log}", LOG)
tmp = SCRIPT + ".run.ps1"
open(tmp, "wb").write(b"\xef\xbb\xbf" + body.encode("utf-8"))

if os.path.exists(LOG):
    os.remove(LOG)

params = '-NoProfile -ExecutionPolicy Bypass -File "' + tmp + '"'
rc = ctypes.windll.shell32.ShellExecuteW(None, "runas", "powershell.exe", params, None, 0)
if rc <= 32:
    print(f"UAC 被取消 (rc={rc})")
    raise SystemExit(2)
print("只读核验已启动（UAC 点『是』），轮询…")
for _ in range(90):
    time.sleep(2)
    if os.path.exists(LOG) and os.path.getsize(LOG) > 0:
        break
raw = open(LOG, "rb").read() if os.path.exists(LOG) else b""
text = ""
for enc in ("utf-16", "gbk", "utf-8"):
    try:
        text = raw.decode(enc)
        break
    except Exception:
        text = raw.decode("utf-8", "replace")
print(text)
