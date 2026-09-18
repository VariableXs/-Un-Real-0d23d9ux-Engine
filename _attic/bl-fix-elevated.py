#!/usr/bin/env python3
"""BL 保命驱动：暂停 BitLocker(无限期) + TPM/SB 全查 + SB 闸门下清 BCD。
链路：补 BOM → PSParser 预检 → ShellExecuteW → 轮询至 ALLDONE。"""
import ctypes
import os
import subprocess
import sys
import time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
SCRIPT = ROOT + r"\_attic\bl-fix.ps1"
LOG = ROOT + r"\_attic\bl-fix.log"
CHECK = ROOT + r"\_attic\bcd-fix2-parse-check.py"

# 补 BOM（PS5.1 无 BOM 按 GBK 读，中文串乱码炸引号）
raw = open(SCRIPT, "rb").read()
if not raw.startswith(b"\xef\xbb\xbf"):
    open(SCRIPT, "wb").write(b"\xef\xbb\xbf" + raw)
    print("BOM added to bl-fix.ps1")

# 提权前语法预检
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

rc = ctypes.windll.shell32.ShellExecuteW(
    None, "runas", "powershell.exe", params, None, 0)
if rc <= 32:
    print(f"UAC 提权被取消或失败 (ShellExecuteW rc={rc})")
    sys.exit(2)


def read_log() -> str:
    with open(LOG, "rb") as f:
        raw = f.read()
    if raw.startswith(b"\xff\xfe"):
        return raw.decode("utf-16", "replace")
    return raw.decode("gbk", "replace")


print("BL 修复已启动（UAC 弹窗点『是』），轮询…")
seen = ""
deadline = time.time() + 120
final = ""
while time.time() < deadline:
    time.sleep(3)
    tail = read_log() if os.path.exists(LOG) else ""
    new = tail[len(seen):]
    if new.strip():
        print(new, end="", flush=True)
        seen = tail
    if "ALLDONE" in tail:
        final = tail
        break
print("\n[驱动] 轮询结束")
if "SUSPEND-OK" in final:
    print("[结果] BitLocker 已无限期暂停")
if "BCD-CLEANED" in final:
    print("[结果] 坏引导项已删除")
elif "SB-STILL-ON" in final:
    print("[结果] SB 仍开，BCD 未动")
