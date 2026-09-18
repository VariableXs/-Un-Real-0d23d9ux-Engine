#!/usr/bin/env python3
"""AI-P 五分区部署预演包装（-PlanOnly 只读不碰盘）。"""
import subprocess
import sys

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
r = subprocess.run(
    ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass",
     "-File", ROOT + r"\portable\AI-P\Deploy-Varix-USB.ps1", "-PlanOnly"],
    capture_output=True, timeout=180, cwd=ROOT,
)
out = r.stdout.decode("gbk", "replace")
print(out[-3500:] if out else "(no stdout)")
err = (r.stderr.decode("gbk", "replace") or "").strip()
if err:
    print("STDERR:", err[-1000:])
print("RC=", r.returncode)
