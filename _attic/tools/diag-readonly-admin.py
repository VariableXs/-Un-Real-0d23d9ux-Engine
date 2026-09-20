#!/usr/bin/env python3
"""提权状态探测（读-only，不写任何东西）。"""
import subprocess

PS = r'''
$id = [Security.Principal.WindowsIdentity]::GetCurrent()
$p = New-Object Security.Principal.WindowsPrincipal($id)
Write-Output ("User: " + $id.Name)
Write-Output ("IsAdmin: " + $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator))
'''

r = subprocess.run(
    ["powershell", "-NoProfile", "-NonInteractive", "-Command", PS],
    capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=60,
)
print((r.stdout or "").strip() or "(no stdout)")
if r.stderr:
    print("stderr:", r.stderr.strip()[:500])
