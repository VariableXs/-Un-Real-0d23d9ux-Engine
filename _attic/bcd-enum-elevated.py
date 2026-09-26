#!/usr/bin/env python3
"""引导项只读枚举的提权驱动。

链路（沿用 _attic/bcd-cleanup-elevated.py 的既有范式）：
  补 BOM → PSParser 预检（0 错才提权）→ ShellExecuteW(runas) → 轮询至 DONE。

为什么每步都不能省：
  - BOM：PS5.1 无 BOM 按 GBK 读 UTF-8 中文，会吞引号导致语法雪崩
  - 预检：语法错了再弹 UAC 是浪费用户一次点击，且失败现场难读
  - `*` 重定向必须在 try 块内部包住 & 调用（PS5.1 语法铁律，语句外跟 `*>` 非法）

**严格只读**：目标脚本只 enum 不写，符合用户第 11/12 条红线。
"""
import ctypes
import os
import subprocess
import sys
import time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
SCRIPT = ROOT + r"\_attic\bcd-enum-readonly.ps1"
LOG = ROOT + r"\_attic\bcd-enum-readonly.log"
CHECK = ROOT + r"\_attic\tools\check-ps-syntax.py"

# 补 BOM（PS5.1 无 BOM 按 GBK 读，中文字符串乱码炸引号）
raw = open(SCRIPT, "rb").read()
if not raw.startswith(b"\xef\xbb\xbf"):
    open(SCRIPT, "wb").write(b"\xef\xbb\xbf" + raw)
    print("BOM added to bcd-enum-readonly.ps1")

# 提权前本地语法预检（别浪费用户 UAC 点击）
r = subprocess.run([sys.executable, CHECK, SCRIPT], capture_output=True)
out = r.stdout.decode("utf-8", "replace") + r.stderr.decode("utf-8", "replace")
print(out)
# 判据必须匹配 check-ps-syntax.py 的真实输出（它打的是 ALL-SYNTAX-OK，
# 不是 ParseErrors: 0 —— 写错判据会把「全绿」误判成「未过」而白白放弃提权）。
if "ALL-SYNTAX-OK" not in out and "SYNTAX-OK" not in out:
    print("[驱动] 语法预检未过，放弃提权")
    sys.exit(3)

if os.path.exists(LOG):
    os.remove(LOG)

# `*>` 必须在 try 块内部包住 & 调用（PS5.1 语法铁律）
params = (
    '-NoProfile -ExecutionPolicy Bypass -Command '
    f'try {{ & \'{SCRIPT}\' *> \'{LOG}\' }} '
    f'catch {{ $_ | Out-String | Add-Content \'{LOG}\' }}'
)

# ShellExecuteW 签名只有 6 个参数
# (hwnd, lpOperation, lpFile, lpParameters, lpDirectory, nShowCmd)。
# 多传一个 None 会让 ctypes 按错误原型调用，rc 直接不可用。
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
    return raw.decode("utf-8", "replace")


print("只读枚举已启动（UAC 弹窗点『是』），轮询…")
seen = ""
deadline = time.time() + 120
final = ""
while time.time() < deadline:
    time.sleep(2)
    tail = read_log() if os.path.exists(LOG) else ""
    new = tail[len(seen):]
    if new.strip():
        print(new, end="", flush=True)
        seen = tail
    if "BCD-ENUM-READONLY DONE" in tail:
        final = tail
        break
print("\n[驱动] 轮询结束")
if not final:
    print("[结果] 未见 DONE 标记 —— 可能 UAC 被拒或脚本中途失败，请查看日志")
