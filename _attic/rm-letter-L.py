#!/usr/bin/env python3
"""清理 L: 盘符残留（diskpart remove letter=L，内核更新后的收尾）。"""
import subprocess

script = "select disk 1\r\nselect partition 1\r\nremove letter=L\r\n"
path = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\_tmp-rm-L.txt"
with open(path, "w", encoding="ascii", newline="") as f:
    f.write(script)

r = subprocess.run(["diskpart", "/s", path], capture_output=True, timeout=120)
print(r.stdout.decode("gbk", "replace"))

# 复核：L: 应该不可见
probe = subprocess.run(
    ["python", "-c",
     "import os; print('L-Test-Path:', os.path.exists('L:\\\\'))"],
    capture_output=True, timeout=30)
print(probe.stdout.decode("gbk", "replace"))
