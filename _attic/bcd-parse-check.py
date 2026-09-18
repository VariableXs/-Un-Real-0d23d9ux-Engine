#!/usr/bin/env python3
"""提权前本地语法验证：PSParser 解析 ps1，0 错才放行提权（不浪费用户 UAC 点击）。"""
import subprocess

PS = r"""
$t = [IO.File]::ReadAllText('D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\bcd-addvarix.ps1')
$errs = $null
[System.Management.Automation.PSParser]::Tokenize($t, [ref]$errs) | Out-Null
Write-Output ("ParseErrors: " + $errs.Count)
$errs | ForEach-Object { Write-Output ("line " + $_.Token.StartLine + ": " + $_.Message) }
"""

r = subprocess.run(["powershell", "-NoProfile", "-Command", PS], capture_output=True)
print(r.stdout.decode("gbk", "replace"))
print(r.stderr.decode("gbk", "replace"))
