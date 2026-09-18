#!/usr/bin/env python3
"""暴力扫描：找出 edk2 固件能被 QEMU(Windows) 加载的传参方式。每个变体跑 3 秒看 rc。"""
import os
import subprocess
import time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
ATTIC = ROOT + r"\_attic"
QEMU = r"C:\Program Files\qemu\qemu-system-x86_64"
FD = ATTIC + r"\edk2-x86_64-code.fd"

VARIANTS = {
    "bios backslash": ["-bios", FD],
    "bios fwdslash": ["-bios", FD.replace("\\", "/")],
    "bios relative": ["-bios", "edk2-x86_64-code.fd"],
    "pflash unit0": ["-drive", f"if=pflash,format=raw,file={FD},unit=0"],
}

base = [QEMU, "-M", "pc", "-m", "256M", "-display", "none",
        "-no-reboot", "-monitor", "none", "-serial", "file:" + ATTIC + r"\sweep-serial.log"]

for name, extra in VARIANTS.items():
    err = ATTIC + r"\sweep-err.log"
    with open(err, "wb") as ef:
        p = subprocess.Popen(base + extra, stdout=ef, stderr=ef, cwd=ATTIC)
        time.sleep(3)
        alive = p.poll() is None
        p.kill()
        p.wait()
    msg = open(err, "r", errors="replace").read().strip().replace("\n", " | ")[:200]
    print(f"{name:16s} alive_after_3s={alive}  stderr: {msg}")
