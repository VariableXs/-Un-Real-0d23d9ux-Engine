# -*- coding: utf-8 -*-
"""查 Disk1/分区X 的存储层只读标志位。"""
import subprocess
import sys

PS = (
    "$ErrorActionPreference='SilentlyContinue'\n"
    "$d = Get-Disk -Number 1\n"
    "Write-Output ('DISK1_IsReadOnly=' + $d.IsReadOnly)\n"
    "$p = Get-Partition -DriveLetter X\n"
    "Write-Output ('PARTX_IsReadOnly=' + $p.IsReadOnly)\n"
)
out = subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command", PS],
                     capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=60)
sys.stdout.write(out.stdout)
if out.stderr:
    sys.stderr.write(out.stderr[-200:])
