#!/usr/bin/env python3
"""S0.4 关闭快速启动（Fast Startup / Hiberboot）——AI-1 · 2026-09-21。

这是三体施工全程**唯一允许的写内置盘操作**（单注册表键、可逆），
前置条件=用户知情确认（《实施总步骤图》S0.4）。运行本脚本即视为
用户确认；脚本本身只做三件事：

  1) 提权写 HKLM\\...\\Power\\HiberbootEnabled = 0 (REG_DWORD)
  2) 提权回读验证 = 0（写后即证，不过就报错退出）
  3) 提示用户手动重启两次后，用 --verify 再回读一次

回滚：HiberbootEnabled = 1 即还原（--restore）。

用法：
    python _attic/hiberboot-off.py            # 写 0 + 回读
    python _attic/hiberboot-off.py --verify   # 重启两次后复核
    python _attic/hiberboot-off.py --restore  # 还原为 1
"""

import ctypes
import os
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCRIPT = os.path.join(ROOT, "_attic", "hiberboot-off.ps1")
LOG = os.path.join(ROOT, "_attic", "hiberboot-off.log")

PS_BODY = r"""
$ErrorActionPreference = 'Stop'
$log = '{log}'
function Say($m) {{ Add-Content -Path $log -Value $m }}
try {{
  $path = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power'
  $val = {value}
  Set-ItemProperty -Path $path -Name HiberbootEnabled -Value $val -Type DWord
  $back = (Get-ItemProperty -Path $path -Name HiberbootEnabled).HiberbootEnabled
  if ($back -ne $val) {{ throw "read-back mismatch: $back" }}
  Say "HIBERBOOT-SET-OK HiberbootEnabled=$back"
}} catch {{
  Say "HIBERBOOT-SET-FAIL $($_.Exception.Message)"
}}
"""


def run_elevated(value: int) -> int:
    body = PS_BODY.format(log=LOG, value=value)
    with open(SCRIPT, "wb") as f:
        f.write(b"\xef\xbb\xbf" + body.encode("ascii"))
    # 语法预检（PSParser 0 错才动手——提权戒律）
    check = subprocess.run(
        ["powershell.exe", "-NoProfile", "-Command",
         f"$e=$null; [System.Management.Automation.PSParser]::Tokenize("
         f"(Get-Content -Raw '{SCRIPT}'), [ref]$e) | Out-Null; "
         f"Write-Output ('ParseErrors: ' + $e.Count)"],
        capture_output=True, text=True)
    out = check.stdout + check.stderr
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
        raise SystemExit(f"UAC 被取消 (rc={rc})——未做任何修改")
    deadline = time.time() + 60
    while time.time() < deadline:
        time.sleep(1)
        if os.path.exists(LOG):
            text = open(LOG, "rb").read().decode("utf-16", "replace")
            if "HIBERBOOT-SET-OK" in text or "HIBERBOOT-SET-FAIL" in text:
                print(text.strip())
                return 0 if "HIBERBOOT-SET-OK" in text else 1
    print("超时——见日志", LOG)
    return 2


def main() -> int:
    if "--restore" in sys.argv:
        print("还原 HiberbootEnabled=1 …")
        return run_elevated(1)
    mode = "verify" if "--verify" in sys.argv else "set"
    print("写入 HiberbootEnabled=0 …" if mode == "set" else "复核 HiberbootEnabled …")
    rc = run_elevated(0)
    if rc == 0 and mode == "set":
        print("\n下一步（需你在场）：重启两次（关机→开机→关机→开机），")
        print("然后运行  python _attic/hiberboot-off.py --verify  复核。")
        print("验证引导语义：关机后再开机应出现引导菜单（不再直跳 Windows）。")
    return rc


if __name__ == "__main__":
    sys.exit(main())
