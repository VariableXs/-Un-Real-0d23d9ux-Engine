#!/usr/bin/env python3
"""只读：查询最近一次开机时间（判断 S0.4 双重启是否已发生）。"""
import subprocess

PS = "(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToString('yyyy-MM-dd HH:mm:ss')"
r = subprocess.run(["powershell.exe", "-NoProfile", "-Command", PS], capture_output=True, timeout=60)
raw = r.stdout + r.stderr
for enc in ("gbk", "utf-8"):
    try:
        print("LastBootUpTime:", raw.decode(enc).strip())
        break
    except Exception:
        continue
