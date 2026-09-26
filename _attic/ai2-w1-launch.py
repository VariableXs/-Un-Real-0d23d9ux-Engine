# -*- coding: utf-8 -*-
"""AI-2 · W1 部署启动器：BOM 转换 → 语法先验 → 提权启动（UAC 由用户确认）→ 后台看日志。

提权走 ShellExecuteW runas（弹出一次 UAC，用户点“是”）；
日志 D:\\VarixDeploy\\w1-deploy.log 由监视器轮询至 DONE/FAIL。
"""
import ctypes
import subprocess
import sys
import time

PS1 = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\portable\engine\W1-Deploy.ps1"
LOG = r"D:\VarixDeploy\w1-deploy.log"
ISO = r"D:\VarixDeploy\Win11_25H2_Chinese_Simplified_x64_v2.iso"

# 1) BOM + CRLF（PS 5.1 纪律）
raw = open(PS1, "rb").read()
text = raw.decode("utf-8-sig").replace("\r\n", "\n").replace("\n", "\r\n")
open(PS1, "wb").write(b"\xef\xbb\xbf" + text.encode("utf-8"))
print("[1] BOM+CRLF done")

# 2) 语法先验（0 错才启动）
PS_CHECK = (
    "$errs = $null\n"
    "[System.Management.Automation.PSParser]::Tokenize((Get-Content -LiteralPath '" + PS1 + "' -Raw), [ref]$errs) | Out-Null\n"
    "Write-Output ('PS_ERRORS=' + $errs.Count)\n"
    "foreach ($e in $errs) { Write-Output ('ERR|' + $e.Token.StartLine + '|' + $e.Message) }\n"
)
out = subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command", PS_CHECK],
                     capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=60)
sys.stdout.write(out.stdout)
if "PS_ERRORS=0" not in out.stdout:
    sys.exit("语法先验未过，拒绝启动")

# 3) 提权启动（UAC 弹窗——用户点是；窗口隐藏防误关，进度全走日志）
params = '-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "{}" -IsoPath "{}"'.format(PS1, ISO)
ret = ctypes.windll.shell32.ShellExecuteW(None, "runas", "powershell.exe", params, None, 0)  # SW_HIDE
if ret <= 32:
    sys.exit(f"提权启动失败（code={ret}，UAC 被取消或被策略拒绝）")
print(f"[2] elevated launched hidden (ShellExecute ret={ret})——UAC 请点“是”")

# 4) 监视日志（每 15s；90 分钟超时）
time.sleep(8)
deadline = time.time() + 90 * 60
last_len = 0
while time.time() < deadline:
    try:
        data = open(LOG, "rb").read().decode("utf-8-sig", "replace")
    except FileNotFoundError:
        data = ""
    if len(data) != last_len:
        last_len = len(data)
        tail = data.strip().splitlines()[-1:] 
        print("[log] " + (tail[0] if tail else "(empty)"), flush=True)
    if "W1-DEPLOY-DONE" in data or "W1-DEPLOY-FAIL" in data:
        print("=== LOG TAIL ===")
        sys.stdout.write(data[-2500:])
        print("=== MONITOR-EXIT ===")
        sys.exit(0 if "W1-DEPLOY-DONE" in data else 1)
    time.sleep(15)
print("=== MONITOR TIMEOUT（90 分钟未结束）===")
sys.exit(2)
