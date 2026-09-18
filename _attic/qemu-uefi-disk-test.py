#!/usr/bin/env python3
r"""0xc000007b 隔离测试 v3：pyfatfs 造真 FAT32 ESP 镜像（含 MBR 分区表），
EDK2 UEFI 固件(pflash) 引导 \EFI\BOOT\BOOTX64.EFI —— 完全等价真机 U 盘固件引导路径。

判定：串口出现 "limine:" → 镜像被固件成功加载运行 → 0xc000007b 的锅在 Windows
bootmgr 链（BCD 中转）环节，而非引导器文件本身。
"""
import inspect
import os
import shutil
import struct
import subprocess
import time

from pyfatfs.PyFat import PyFat
from pyfatfs.PyFatFS import PyFatFS

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
ATTIC = ROOT + r"\_attic"
# v4：改用 uefi-vhd-build.ps1 产出的 uefi-esp.vhd（GPT+真 ESP+Windows 原生 FAT32，QEMU format=vpc）
DISK = ATTIC + r"\uefi-esp.vhd"
SERIAL = ATTIC + r"\uefi-test-serial.log"
ERRLOG = ATTIC + r"\uefi-test-stderr.log"
MON_PORT = 14461
QEMU = r"C:\Program Files\qemu\qemu-system-x86_64"
EDK2 = ATTIC + r"\edk2-x86_64-code.fd"

for f in (SERIAL, ERRLOG):
    if os.path.exists(f):
        os.remove(f)

if not os.path.exists(DISK):
    raise SystemExit("!! 先跑 uefi-vhd-build-elevated.py 生成 uefi-esp.vhd")
print(f"[v4] using VHD: {DISK} ({os.path.getsize(DISK):,} B)")

# QEMU + EDK2(pflash) 引导 VHD
proc = subprocess.Popen(
    [
        QEMU,
        "-M", "pc", "-m", "512M",
        "-drive", f"if=pflash,format=raw,file={EDK2},unit=0",
        "-drive", f"file={DISK},format=vpc,if=ide,index=0",
        "-boot", "order=c",
        "-serial", f"file:{SERIAL}",
        "-display", "none", "-no-reboot", "-no-shutdown",
        "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait",
    ],
    stdout=open(ERRLOG, "wb"), stderr=subprocess.STDOUT,
)
print(f"[v4] QEMU pid={proc.pid}")

verdict = ""
seen = ""
deadline = time.time() + 150
while time.time() < deadline:
    time.sleep(2)
    if proc.poll() is not None:
        print(f"[v3] QEMU exited rc={proc.returncode}")
        break
    if os.path.exists(SERIAL):
        tail = open(SERIAL, "r", errors="replace").read()
        if tail != seen:
            print(tail[len(seen):], end="", flush=True)
            seen = tail
        if "limine:" in tail:
            verdict = "LIMINE-RAN"
            break

if verdict:
    time.sleep(10)
    tail = open(SERIAL, "r", errors="replace").read() if os.path.exists(SERIAL) else ""
    print(tail[len(seen):], end="", flush=True)

try:
    import socket
    s = socket.create_connection(("127.0.0.1", MON_PORT), timeout=3)
    s.sendall(b"quit\n")
    time.sleep(1)
    s.close()
except OSError:
    pass
try:
    proc.wait(timeout=5)
except subprocess.TimeoutExpired:
    proc.kill()

print("\n[v3] verdict:", verdict or "NO-LIMINE-OUTPUT")
if os.path.exists(ERRLOG) and os.path.getsize(ERRLOG):
    print("[v3] qemu stderr:", open(ERRLOG, "r", errors="replace").read()[:800])
