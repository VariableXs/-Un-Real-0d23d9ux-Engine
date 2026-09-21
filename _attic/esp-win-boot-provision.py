#!/usr/bin/env python3
"""S1.3 U 盘 ESP Windows 引导供应驱动 v2：BOM→PSParser 预检→占位符注入→提权→轮询。

前置（1.8 SOP）：
  - SecureBoot 须为 Disabled（脚本内部权威判定，True 即自毁）；
  - HiberbootEnabled=0（脚本内部校验）；
  - 用户知情确认：本步在固件 NVRAM 追加一条 U 盘 Windows 引导项（/addlast 追加尾位，
    可逆：删除对应 Boot#### 即还原），只写 U 盘 ESP 与 SHARED；内置盘 ESP 四件套
    哈希前后双录核（变了即 FAIL）。
用法：python _attic/esp-win-boot-provision.py
"""

import ctypes
import os
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "esp-win-boot-provision.ps1")
LOG = os.path.join(HERE, "esp-win-boot-provision.log")

if not os.path.isfile(SCRIPT):
    raise SystemExit("missing " + SCRIPT)

raw = open(SCRIPT, "rb").read()
if not raw.startswith(b"\xef\xbb\xbf"):
    open(SCRIPT, "wb").write(b"\xef\xbb\xbf" + raw)
    print("BOM added")

chk = subprocess = __import__("subprocess").run(
    ["powershell.exe", "-NoProfile", "-Command",
     f"$e=$null; [System.Management.Automation.PSParser]::Tokenize("
     f"(Get-Content -Raw '{SCRIPT}'), [ref]$e) | Out-Null; "
     f"Write-Output ('ParseErrors: ' + $e.Count)"],
    capture_output=True, timeout=60)
out = (chk.stdout or b"") + (chk.stderr or b"")
text = ""
for enc in ("gbk", "utf-8"):
    try:
        text = out.decode(enc)
        break
    except Exception:
        text = out.decode("utf-8", "replace")
if "ParseErrors: 0" not in text:
    print(text)
    raise SystemExit("!! ps1 语法预检未过")

body = open(SCRIPT, "rb").read().decode("utf-8-sig").replace("{log}", LOG)
tmp = SCRIPT + ".run.ps1"
open(tmp, "wb").write(b"\xef\xbb\xbf" + body.encode("utf-8"))

if os.path.exists(LOG):
    os.remove(LOG)

params = '-NoProfile -ExecutionPolicy Bypass -File "' + tmp + '"'
rc = ctypes.windll.shell32.ShellExecuteW(None, "runas", "powershell.exe", params, None, 0)
if rc <= 32:
    print(f"UAC 被取消 (rc={rc})——未做任何修改")
    raise SystemExit(2)

print("S1.3 供应已启动（UAC 点『是』），轮询…（bcdboot 约需 1-2 分钟）")
deadline = time.time() + 600
final = ""
while time.time() < deadline:
    time.sleep(3)
    if os.path.exists(LOG) and os.path.getsize(LOG) > 0:
        raw = open(LOG, "rb").read()
        text = ""
        for enc in ("utf-16", "gbk", "utf-8"):
            try:
                text = raw.decode(enc)
                break
            except Exception:
                text = raw.decode("utf-8", "replace")
        if "WIN-BOOT-PROVISION-DONE" in text or "WIN-BOOT-PROVISION-FAIL" in text:
            final = text
            break
print(final if final else "超时——见日志文件 " + LOG)
