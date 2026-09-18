#!/usr/bin/env python3
"""引导诊断提权驱动：Secure Boot 状态 + 固件启动项列表，落 diag-boot.log 轮询。"""
import ctypes
import os
import sys
import time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
LOG = ROOT + r"\_attic\diag-boot.log"

# try/catch 兜异常，语句级 *> 落日志（PS5.1 默认 UTF-16LE）
simple = (
    '-NoProfile -ExecutionPolicy Bypass -Command '
    f'try {{ '
    f'  try {{ Write-Output ("SecureBoot: " + (Confirm-SecureBootUEFI)) }} '
    f'  catch {{ Write-Output ("SecureBoot: check failed - " + $_.Exception.Message) }}; '
    f'  Write-Output "=== Firmware boot entries ==="; '
    f'  (bcdedit /enum firmware 2>&1 | Out-String) | Write-Output; '
    f'  Write-Output "DIAG DONE" '
    f'}} catch {{ $_ | Out-String }} *> \'{LOG}\''
)

if os.path.exists(LOG):
    os.remove(LOG)

rc = ctypes.windll.shell32.ShellExecuteW(
    None, "runas", "powershell.exe", simple, None, 0)  # SW_HIDE=0
if rc <= 32:
    print(f"UAC 提权被取消或失败 (ShellExecuteW rc={rc})")
    sys.exit(2)


def read_log() -> str:
    with open(LOG, "rb") as f:
        raw = f.read()
    if raw.startswith(b"\xff\xfe"):
        return raw.decode("utf-16", "replace")
    return raw.decode("gbk", "replace")


print("引导诊断已启动（UAC 弹窗点『是』），轮询…")
seen = ""
deadline = time.time() + 120
while time.time() < deadline:
    time.sleep(3)
    tail = read_log() if os.path.exists(LOG) else ""
    new = tail[len(seen):]
    if new.strip():
        print(new, end="", flush=True)
        seen = tail
    if "DIAG DONE" in tail:
        break
print("\n[驱动] 诊断轮询结束")
