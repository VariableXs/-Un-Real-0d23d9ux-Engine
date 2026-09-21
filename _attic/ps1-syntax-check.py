#!/usr/bin/env python3
"""通用 .ps1 语法预检：PSParser 0 错才算过（提权戒律）。用法：ps1-syntax-check.py <path>"""
import subprocess
import sys

if len(sys.argv) < 2:
    raise SystemExit("usage: ps1-syntax-check.py <file.ps1>")
target = sys.argv[1]
cmd = (f"$e=$null; [System.Management.Automation.PSParser]::Tokenize("
       f"(Get-Content -Raw '{target}'), [ref]$e) | Out-Null; "
       f"$e | ForEach-Object {{ Write-Output ('ERR: ' + $_.Token + ' @' + $_.StartLine) }}; "
       f"Write-Output ('ParseErrors: ' + $e.Count)")
r = subprocess.run(["powershell.exe", "-NoProfile", "-Command", cmd], capture_output=True, timeout=60)
raw = (r.stdout or b"") + (r.stderr or b"")
for enc in ("gbk", "utf-8"):
    try:
        print(raw.decode(enc))
        break
    except Exception:
        print(raw.decode("utf-8", "replace"))
        break
