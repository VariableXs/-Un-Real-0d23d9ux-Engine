#!/usr/bin/env python3
r"""贴实机配置 QEMU 冒烟：Y7000 = i7-13650HX(14C) + 32GB + x2APIC。
用现有 uefi-esp.vhd（NXE 修复版内核），验证 SMP 14 核 + 大内存启动路径。
判据：smp: 14 core(s) online。"""
import os
import socket
import subprocess
import time

ATTIC = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic"
DISK = ATTIC + r"\uefi-esp.vhd"
SERIAL = ATTIC + r"\realprofile-serial.log"
ERRLOG = ATTIC + r"\realprofile-stderr.log"
QEMU = r"C:\Program Files\qemu\qemu-system-x86_64"
EDK2 = ATTIC + r"\edk2-x86_64-code.fd"

if os.path.exists(SERIAL):
    os.remove(SERIAL)
proc = subprocess.Popen(
    [
        QEMU,
        "-M", "pc", "-m", "32G",
        "-smp", "14",
        "-cpu", "max",
        "-drive", f"if=pflash,format=raw,file={EDK2},unit=0",
        "-drive", f"file={DISK},format=vpc,if=ide,index=0",
        "-boot", "order=c",
        "-serial", f"file:{SERIAL}",
        "-display", "none", "-no-reboot", "-no-shutdown",
    ],
    stdout=open(ERRLOG, "wb"), stderr=subprocess.STDOUT,
)
print(f"[smoke] QEMU pid={proc.pid} (-smp 14 -m 32G -cpu max)")
verdict = ""
deadline = time.time() + 300
while time.time() < deadline:
    time.sleep(4)
    if proc.poll() is not None:
        print(f"[smoke] exited rc={proc.returncode}")
        break
    if os.path.exists(SERIAL):
        tail = open(SERIAL, "r", errors="replace").read()
        if "smp: 14 core(s) online" in tail:
            verdict = "SMP14-ONLINE"
            break
        if "boot completed" in tail or "clock: source=tsc" in tail:
            seen = tail
try:
    s = socket.create_connection(("127.0.0.1", 14466), timeout=2)
    s.sendall(b"quit\n")
    s.close()
except OSError:
    pass
try:
    proc.wait(timeout=8)
except subprocess.TimeoutExpired:
    proc.kill()

tail = open(SERIAL, "r", errors="replace").read() if os.path.exists(SERIAL) else ""
lines = [ln for ln in tail.splitlines() if ln.strip()]
print("---- serial tail ----")
for ln in lines[-25:]:
    print(ln)
if "smp: 14 core(s) online" in tail:
    print("[smoke] verdict: SMP14-ONLINE（14 核全上，贴实机配置跑通）")
elif "smp:" in tail:
    print("[smoke] verdict: PARTIAL（smp 线见上）")
elif "clock: source=tsc" in tail:
    print("[smoke] verdict: STUCK-AFTER-CLOCK（14 核路径复现卡点）")
else:
    print("[smoke] verdict: NO-BOOT")
