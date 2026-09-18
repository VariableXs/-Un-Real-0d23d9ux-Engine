#!/usr/bin/env python3
"""查询机器完整产品名（定位 Novo 键位置用）。"""
import subprocess

r = subprocess.run(
    ["powershell", "-NoProfile", "-Command",
     "(Get-WmiObject Win32_ComputerSystemProduct).Name; "
     "(Get-WmiObject Win32_ComputerSystemProduct).Version"],
    capture_output=True,
)
print(r.stdout.decode("gbk", "replace"))
print(r.stderr.decode("gbk", "replace"))
