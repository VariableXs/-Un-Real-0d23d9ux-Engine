#!/usr/bin/env python3
"""0xc000007b 隔离测试：UEFI 固件(edk2)直接引导 isoroot（vvfat 虚拟 FAT 磁盘）。

判定逻辑：
- 串口出现 "limine:" 行 → BOOTX64.EFI 被固件成功加载并运行 → 镜像没问题，
  问题出在 Windows bootmgr 链（BCD 中转）这一环节。
- 串口全空 → UEFI 固件也加载不了（或 vvfat 缺陷）→ 需换真 FAT 镜像复测。
"""
import os
import shutil
import subprocess
import sys
import time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
ISODIR = ROOT + r"\build\isoroot"
SERIAL = ROOT + r"\_attic\uefi-test-serial.log"
MON_PORT = 14461
QEMU = r"C:\Program Files\qemu\qemu-system-x86_64"
EDK2 = ROOT + r"\_attic\edk2-x86_64-code.fd"

if os.path.exists(SERIAL):
    os.remove(SERIAL)
ERRLOG = ROOT + r"\_attic\uefi-test-stderr.log"
if os.path.exists(ERRLOG):
    os.remove(ERRLOG)

# isoroot 拷到临时卷目录（vvfat rw 会把客人写入同步回来，不能直接用真 isoroot）
VOLDIR = ROOT + r"\_attic\uefi-test-vol"
if os.path.exists(VOLDIR):
    import shutil
    shutil.rmtree(VOLDIR)
shutil.copytree(ISODIR, VOLDIR)

# vvfat：把目录即时变成 FAT32 虚拟磁盘（rw 模式，落在临时副本上）
vvfat = "fat:32:rw:" + VOLDIR.replace("\\", "/")

errf = open(ERRLOG, "wb")
proc = subprocess.Popen(
    [
        QEMU,
        "-M", "pc", "-m", "512M",
        "-drive", f"if=pflash,format=raw,file={EDK2},unit=0",
        "-drive", f"file={vvfat},format=raw,if=ide,index=0",
        "-boot", "order=c",
        "-serial", f"file:{SERIAL}",
        "-display", "none", "-no-reboot", "-no-shutdown",
        "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait",
    ],
    stdout=errf, stderr=errf,
)
print(f"[uefi-test] QEMU pid={proc.pid} started (vvfat={vvfat})")

MARKERS = [
    "limine: Loading executable",
    "limine:",
    "[ INFO]",
]
deadline = time.time() + 120
seen = ""
verdict = ""
while time.time() < deadline:
    time.sleep(2)
    if proc.poll() is not None:
        print(f"[uefi-test] QEMU exited rc={proc.returncode}")
        break
    if os.path.exists(SERIAL):
        with open(SERIAL, "r", errors="replace") as f:
            tail = f.read()
        if tail != seen:
            print(tail[len(seen):], end="", flush=True)
            seen = tail
        if MARKERS[0] in tail:
            verdict = "LIMINE-RAN"
            break
        if "limine:" in tail:
            verdict = "LIMINE-RAN"
            break

# 再多等 8s 抓后续内核输出
if verdict:
    time.sleep(8)
    if os.path.exists(SERIAL):
        with open(SERIAL, "r", errors="replace") as f:
            tail = f.read()
        print(tail[len(seen):], end="", flush=True)
        seen = tail

# 关机
try:
    import socket
    s = socket.create_connection(("127.0.0.1", MON_PORT), timeout=3)
    s.sendall(b"quit\n")
    time.sleep(1)
    s.close()
except OSError:
    proc.kill()
try:
    proc.wait(timeout=5)
except subprocess.TimeoutExpired:
    proc.kill()

print("\n[uefi-test] final serial size:", os.path.getsize(SERIAL) if os.path.exists(SERIAL) else 0)
print("[uefi-test] verdict:", verdict or "NO-LIMINE-OUTPUT")
errf.close()
if os.path.exists(ERRLOG) and os.path.getsize(ERRLOG):
    print("[uefi-test] qemu stderr:")
    print(open(ERRLOG, "r", errors="replace").read()[:2000])
