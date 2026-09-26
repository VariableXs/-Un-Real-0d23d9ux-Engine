# -*- coding: utf-8 -*-
"""AI-2 · W1 收尾启动器：BOM/语法 → 提权运行 W1-Finalize.ps1（一次 UAC）→ 监视日志终态。"""
import ctypes
import subprocess
import sys
import time

PS1 = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\portable\engine\W1-Finalize.ps1"
LOG = r"D:\VarixDeploy\w1-finalize.log"

raw = open(PS1, "rb").read()
text = raw.decode("utf-8-sig").replace("\r\n", "\n").replace("\n", "\r\n")
open(PS1, "wb").write(b"\xef\xbb\xbf" + text.encode("utf-8"))
print("[1] BOM+CRLF done", flush=True)

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

params = '-NoProfile -ExecutionPolicy Bypass -File "{}"'.format(PS1)
ret = ctypes.windll.shell32.ShellExecuteW(None, "runas", "powershell.exe", params, None, 0)
if ret <= 32:
    sys.exit(f"提权启动失败（code={ret}）")
print("[2] finalize launched hidden（UAC 请点是）", flush=True)

deadline = time.time() + 60 * 60
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
    if "W1-FINALIZE-DONE" in data:
        print("=== W1-FINALIZE-DONE ===", flush=True)
        sys.stdout.write(data[-1500:])
        sys.exit(0)
    if "W1-FINALIZE-FAIL" in data:
        print("=== W1-FINALIZE-FAIL ===", flush=True)
        sys.stdout.write(data[-2000:])
        sys.exit(1)
    time.sleep(15)
print("=== MONITOR TIMEOUT 60min ===")
sys.exit(2)
