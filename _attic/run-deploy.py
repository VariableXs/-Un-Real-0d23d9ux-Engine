#!/usr/bin/env python3
"""esp-deploy-switch.ps1 的 python 包装（项目戒律：PowerShell 工具空输出时
用 python subprocess 包一层跑；输出落文件避免编码丢失）。"""
import subprocess
import sys

r = subprocess.run(
    ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass",
     "-File", r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\esp-deploy-switch.ps1"],
    capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=300,
)
out = (r.stdout or "") + (("\n[stderr] " + r.stderr) if r.stderr else "")
sys.stdout.write(out)
sys.stdout.write(f"\n[exit] {r.returncode}\n")
with open(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\deploy-last-output.txt", "w", encoding="utf-8") as f:
    f.write(out)
raise SystemExit(r.returncode)
