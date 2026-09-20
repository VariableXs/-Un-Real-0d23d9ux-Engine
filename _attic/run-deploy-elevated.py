#!/usr/bin/env python3
"""esp-deploy-switch.ps1 提权执行器（项目戒律范式）：
1) PSParser 本地语法 0 错才放行（不浪费用户 UAC 点击）；
2) ShellExecuteW runas 提权，*> 重定向落日志文件（*> 包在 try 内）；
3) 轮询日志文件出现完成/失败标记后退出。"""
import subprocess
import sys
import time

SCRIPT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\esp-deploy-switch.ps1"
LOG = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\deploy-elevated-output.txt"

# 1) 语法检查（非提权）
PS_CHECK = (
    "$t = [IO.File]::ReadAllText('" + SCRIPT + "');"
    "$errs = $null;"
    "[System.Management.Automation.PSParser]::Tokenize($t, [ref]$errs) | Out-Null;"
    "Write-Output ('ParseErrors: ' + $errs.Count)"
)
r = subprocess.run(["powershell", "-NoProfile", "-Command", PS_CHECK], capture_output=True)
out = r.stdout.decode("gbk", "replace")
print(out.strip())
if "ParseErrors: 0" not in out:
    print("语法检查未过，放弃提权")
    raise SystemExit(2)

# 2) 提权执行（*> 包在 try 内——PS5.1 语法戒律）
PS_ELEV = (
    "try { & '" + SCRIPT + "' *> '" + LOG + "' } catch { "
    "[IO.File]::AppendAllText('" + LOG + "', 'ELEV-CAUGHT: ' + $_.Exception.Message) }"
)
import ctypes
rc = ctypes.windll.shell32.ShellExecuteW(
    None, "runas", "powershell.exe",
    "-NoProfile -ExecutionPolicy Bypass -Command " + '"' + PS_ELEV.replace('"', '\\"') + '"',
    None, 0,  # SW_HIDE
)
print("ShellExecuteW rc =", rc)
if rc <= 32:
    print("提权启动失败（用户取消 UAC 或策略拒绝）")
    raise SystemExit(3)

# 3) 轮询日志
print("等待部署日志…（UAC 弹窗请点「是」）")


def read_log_text():
    """PS `*>` 重定向默认 UTF-16LE（BOM FF FE）——按 BOM 嗅探解码，
    否则按 UTF-8。之前恒按 utf-8 读 UTF-16 日志，标记永远匹配不到
    （实测部署已 DONE 却误报 120s 超时）。"""
    raw = open(LOG, "rb").read()
    if raw[:2] in (b"\xff\xfe", b"\xfe\xff"):
        return raw.decode("utf-16", errors="replace")
    return raw.decode("utf-8", errors="replace")


for i in range(120):
    time.sleep(2)
    try:
        txt = read_log_text()
        if "ESP-DEPLOY-DONE" in txt or "DEPLOY-FAIL" in txt:
            print(txt)
            raise SystemExit(0 if "ESP-DEPLOY-DONE" in txt else 1)
    except FileNotFoundError:
        pass
print("超时：120s 内未出现完成/失败标记")
raise SystemExit(4)
