# -*- coding: utf-8 -*-
"""AI-2 · W1 收尾启动器 v3（计划任务模式）：setup 建任务 VarixW1Finalize 并 /RUN；监视器锚定最后一轮开始。"""
import ctypes
import subprocess
import sys
import time

PS1 = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\portable\engine\W1-Finalize.ps1"
LOG = r"D:\VarixDeploy\w1-finalize.log"
SETUP = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\ai2-w1-finalize-setup.ps1"
TASK = "VarixW1Finalize"

# 1) BOM + 语法先验
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
    sys.exit("语法先验未过")

# 2) setup 脚本
setup_text = (
    "$ErrorActionPreference='Stop'\n"
    "schtasks /Create /TN '" + TASK + "' /TR \"powershell.exe -NoProfile -ExecutionPolicy Bypass -File '" + PS1 + "'\" "
    "/SC ONCE /ST 23:59 /RL HIGHEST /F | Out-Null\n"
    "if ($LASTEXITCODE -ne 0) { throw ('create failed ' + $LASTEXITCODE) }\n"
    "schtasks /Run /TN '" + TASK + "' | Out-Null\n"
    "if ($LASTEXITCODE -ne 0) { throw ('run failed ' + $LASTEXITCODE) }\n"
    "Write-Output 'SETUP-OK'\n"
)
open(SETUP, "wb").write(b"\xef\xbb\xbf" + setup_text.replace("\n", "\r\n").encode("utf-8"))

# 3) 提权 setup（一次 UAC）
params = '-NoProfile -ExecutionPolicy Bypass -File "{}"'.format(SETUP)
ret = ctypes.windll.shell32.ShellExecuteW(None, "runas", "powershell.exe", params, None, 0)
if ret <= 32:
    sys.exit(f"提权启动失败（code={ret}）")
print("[2] setup launched（UAC 请点是）", flush=True)
time.sleep(10)

# 4) 锚定监视（150 分钟）
deadline = time.time() + 150 * 60
last_session_len = -1
while time.time() < deadline:
    try:
        data = open(LOG, "rb").read().decode("utf-8-sig", "replace")
    except FileNotFoundError:
        data = ""
    anchor = data.rfind("W1 收尾开始")
    session = data[anchor:] if anchor >= 0 else ""
    if len(session) != last_session_len:
        last_session_len = len(session)
        lines = [l for l in session.strip().splitlines() if l.startswith("[")]
        print("[log] " + (lines[-1] if lines else "(waiting new run...)"), flush=True)
    if session and "W1-FINALIZE-DONE" in session:
        print("=== W1-FINALIZE-DONE ===", flush=True)
        sys.stdout.write(session[-1200:])
        sys.exit(0)
    if session and "W1-FINALIZE-FAIL" in session:
        print("=== W1-FINALIZE-FAIL ===", flush=True)
        sys.stdout.write(session[-1500:])
        sys.exit(1)
    time.sleep(15)
print("=== MONITOR TIMEOUT ===")
sys.exit(2)
