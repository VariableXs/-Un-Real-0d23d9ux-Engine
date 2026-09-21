# -*- coding: utf-8 -*-
"""S1.1 辅助：各卷已用空间 + 文件总量盘点（只读）。"""
import os
import subprocess
import sys

PS = r'''
Get-CimInstance Win32_LogicalDisk -Filter "DriveType=3" | ForEach-Object { Write-Output ("VOL|" + $_.DeviceID + "|" + $_.VolumeName + "|used=" + [math]::Round(($_.Size-$_.FreeSpace)/1GB,2) + "GB|free=" + [math]::Round($_.FreeSpace/1GB,1) + "GB") }
'''
out = subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command", PS],
                     capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=120)
sys.stdout.write(out.stdout)

for vol in ["S:\\", "W:\\", "V:\\", "X:\\"]:
    n = 0
    tot = 0
    for dp, dn, fn in os.walk(vol):
        dn[:] = [d for d in dn if not d.startswith("$")]
        for f in fn:
            if f.startswith("$"):
                continue
            try:
                s = os.stat(os.path.join(dp, f), follow_symlinks=False).st_size
                n += 1
                tot += s
            except OSError:
                pass
    print(f"{vol} files={n} total={tot/1024/1024:.1f}MiB")
