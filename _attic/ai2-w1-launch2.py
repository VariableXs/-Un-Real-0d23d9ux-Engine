# -*- coding: utf-8 -*-
"""AI-2 · W1 部署启动器 v2（计划任务模式）。

流程：BOM/语法先验 → 写 setup 脚本 → 提权运行 setup（一次 UAC）：
  setup = schtasks 创建 VarixW1Deploy 任务（RL HIGHEST，指向 W1-Deploy.ps1）→ /RUN → 退出。
部署在 Task Scheduler 会话 0 独立执行（无窗口、免疫父进程死亡）。
本脚本随后轮询日志至 DONE/FAIL（90 分钟超时）。
"""
import ctypes
import subprocess
import sys
import time

PS1 = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\portable\engine\W1-Deploy.ps1"
LOG = r"D:\VarixDeploy\w1-deploy.log"
ISO = r"D:\VarixDeploy\Win11_25H2_Chinese_Simplified_x64_v2.iso"
SETUP = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\ai2-w1-setup.ps1"
TASK = "VarixW1Deploy"

# 1) BOM + CRLF + 语法先验
raw = open(PS1, "rb").read()
text = raw.decode("utf-8-sig").replace("\r\n", "\n").replace("\n", "\r\n")
open(PS1, "wb").write(b"\xef\xbb\xbf" + text.encode("utf-8"))
print("[1] BOM+CRLF done", flush=True)
PS_CHECK = (
    "$errs = $null\n"
    "[System.Management.Automation.PSParser]::Tokenize((Get-Content -LiteralPath '" + PS1 + "' -Raw), [ref]$errs) | Out-Null\n"
    "Write-Output ('PS_ERRORS=' + $errs.Count)\n"
)
out = subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command", PS_CHECK],
                     capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=60)
sys.stdout.write(out.stdout)
if "PS_ERRORS=0" not in out.stdout:
    sys.exit("语法先验未过，拒绝启动")

# 2) setup 脚本（提权一次性：建任务+运行+退出）
setup_text = (
    "$ErrorActionPreference='Stop'\n"
    "schtasks /Create /TN '" + TASK + "' /TR \"powershell.exe -NoProfile -ExecutionPolicy Bypass -File '" + PS1 + "' -IsoPath '" + ISO + "'\" "
    "/SC ONCE /ST 23:59 /RL HIGHEST /F | Out-Null\n"
    "if ($LASTEXITCODE -ne 0) { throw ('schtasks create failed ' + $LASTEXITCODE) }\n"
    "schtasks /Run /TN '" + TASK + "' | Out-Null\n"
    "if ($LASTEXITCODE -ne 0) { throw ('schtasks run failed ' + $LASTEXITCODE) }\n"
    "Write-Output 'SETUP-OK task created and started'\n"
)
open(SETUP, "wb").write(b"\xef\xbb\xbf" + setup_text.replace("\n", "\r\n").encode("utf-8"))

# 3) 提权运行 setup（一次 UAC）
params = '-NoProfile -ExecutionPolicy Bypass -File "{}"'.format(SETUP)
ret = ctypes.windll.shell32.ShellExecuteW(None, "runas", "powershell.exe", params, None, 0)
if ret <= 32:
    sys.exit(f"提权启动失败（code={ret}）")
print("[2] setup launched (UAC 请点是)", flush=True)
time.sleep(10)

# 4) 轮询日志（90 分钟）
deadline = time.time() + 90 * 60
last_len = 0
while time.time() < deadline:
    try:
        data = open(LOG, "rb").read().decode("utf-8-sig", "replace")
    except FileNotFoundError:
        data = ""
    if len(data) != last_len:
        last_len = len(data)
        lines = [l for l in data.strip().splitlines() if l.startswith("[")]
        print("[log] " + (lines[-1] if lines else "(empty)"), flush=True)
    if "W1-DEPLOY-DONE" in data:
        print("=== DONE ===")
        sys.stdout.write(data[-1800:])
        sys.exit(0)
    if "W1-DEPLOY-FAIL" in data:
        print("=== FAIL ===")
        sys.stdout.write(data[-1800:])
        sys.exit(1)
    time.sleep(15)
print("=== MONITOR TIMEOUT ===")
sys.exit(2)
