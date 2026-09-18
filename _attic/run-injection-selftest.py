# -*- coding: utf-8 -*-
"""Run Engine-Input-Injection.ps1 self-test via subprocess (PS tool empty-output workaround)."""
import subprocess
import sys

r = subprocess.run(
    ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass",
     "-File", r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\portable\AI-P\Engine-Input-Injection.ps1",
     "-SelfTest", "-TypingSeconds", "3"],
    capture_output=True, text=True, errors="replace", timeout=110,
)
print("exit:", r.returncode)
print("--- stdout ---")
print(r.stdout)
print("--- stderr ---")
print(r.stderr[:2000])
sys.exit(0 if r.returncode == 0 else 1)
