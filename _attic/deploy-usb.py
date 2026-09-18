#!/usr/bin/env python3
"""U 盘五分区整机部署驱动（提权 + 日志轮询）。

用户已明确授权清盘（TU200Pro 1T，Disk 1，非启动/系统盘——脚本 Preflight
另有 IsBoot/IsSystem 物理隔离保护）。提权进程输出落 deploy-full.log，
主进程轮询日志与 deploy-state.json 跟进度。
"""
import ctypes
import os
import subprocess
import sys
import time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
SCRIPT = ROOT + r"\portable\AI-P\Deploy-Varix-USB.ps1"
SYSSRC = ROOT + r"\_attic\syssource"
LOG = ROOT + r"\_attic\deploy-full.log"
STATE = ROOT + r"\_attic\deploy-state\deploy-state.json"

params = (
    '-NoProfile -ExecutionPolicy Bypass -Command '
    # try/catch：终止异常（如参数绑定错误）冒泡时绕过 *> 重定向，必须显式落日志
    f'try {{ & \'{SCRIPT}\' -DiskNumber 1 -Yes -SysSource \'{SYSSRC}\' *> \'{LOG}\' }} '
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

print("提权部署已启动（UAC 弹窗点『是』），轮询进度…")
seen = ""
deadline = time.time() + 900
while time.time() < deadline:
    time.sleep(5)
    tail = read_log() if os.path.exists(LOG) else ""
    new = tail[len(seen):]
    if new.strip():
        print(new, end="", flush=True)
        seen = tail
    # 完成判据：Report 阶段归档后 ps1 收尾行含「五分区部署编排完成」
    if "部署编排完成" in tail:
        break
    if "throw" in tail.lower() or "失败" in tail:
        print("\n[驱动] 检测到失败标志，停止轮询")
        break
print("\n[驱动] 轮询结束")
