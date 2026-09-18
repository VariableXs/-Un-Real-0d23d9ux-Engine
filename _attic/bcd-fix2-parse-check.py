#!/usr/bin/env python3
"""提权前本地语法验证：PSParser 解析 ps1，0 错才放行提权。用法: python bcd-fix2-parse-check.py <script.ps1>"""
import subprocess
import sys

TARGET = sys.argv[1] if len(sys.argv) > 1 else (
    r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\bcd-fix2.ps1")

PS = (
    "$t = [IO.File]::ReadAllText('" + TARGET + "')\n"
    "$errs = $null\n"
    "[System.Management.Automation.PSParser]::Tokenize($t, [ref]$errs) | Out-Null\n"
    "Write-Output (\"ParseErrors: \" + $errs.Count)\n"
    "$errs | ForEach-Object { Write-Output (\"line \" + $_.Token.StartLine + \": \" + $_.Message) }\n"
)

r = subprocess.run(["powershell", "-NoProfile", "-Command", PS], capture_output=True)
print(r.stdout.decode("gbk", "replace"))
print(r.stderr.decode("gbk", "replace"))
