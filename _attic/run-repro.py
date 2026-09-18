# -*- coding: utf-8 -*-
import subprocess
r = subprocess.run(
    ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass",
     "-File", r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\mock-handshake-repro.ps1"],
    capture_output=True, text=True, errors="replace", timeout=60,
)
print("exit:", r.returncode)
print(r.stdout)
print("stderr:", r.stderr[:800])
