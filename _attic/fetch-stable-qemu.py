# -*- coding: utf-8 -*-
"""下载稳定版 QEMU → 静默安装到 _attic/qemu-stable → 报版本。"""
import subprocess
import sys
import time
from pathlib import Path

ATTIC = Path(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic")
URL = "https://qemu.weilnetz.de/w64/qemu-w64-setup-20260811.exe"
EXE = ATTIC / "qemu-stable-setup.exe"
DEST = ATTIC / "qemu-stable"
QEMU_SYS = DEST / "qemu-system-x86_64.exe"

# 断点续传直到完整（远端大小 206615928）。
TARGET = 206615928
for attempt in range(30):
    size = EXE.stat().st_size if EXE.exists() else 0
    if size >= TARGET:
        break
    print(f"resume from {size}...", flush=True)
    subprocess.run(
        ["curl", "-L", "-C", "-", "--max-time", "560", "-o", str(EXE), URL],
        cwd=str(ATTIC), capture_output=True,
    )
else:
    print("DOWNLOAD FAILED after retries")
    sys.exit(2)
print(f"download complete: {EXE.stat().st_size}")

# InnoSetup 静默安装。
r = subprocess.run(
    [str(EXE), "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", f"/DIR={DEST}"],
    cwd=str(ATTIC), capture_output=True, text=True,
)
print("installer rc=", r.returncode)
if QEMU_SYS.exists():
    v = subprocess.run([str(QEMU_SYS), "--version"], capture_output=True, text=True)
    print(v.stdout.strip())
else:
    print("qemu-system-x86_64.exe missing after install")
    sys.exit(3)
print("ALL DONE")
