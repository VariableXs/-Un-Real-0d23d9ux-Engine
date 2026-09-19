#!/usr/bin/env python3
r"""CPU#1 单步取证：stop → cpu 1 → 逐条 stepi，抓第一条不放行的指令。
同时用 CPU#1 的 MMU 读栈与入口代码，验证映射。"""
import os
import socket
import subprocess
import time

ATTIC = os.path.dirname(os.path.abspath(__file__))
DISK = ATTIC + r"\uefi-esp.vhd"
SERIAL = ATTIC + r"\step-probe-serial.log"
ERRLOG = ATTIC + r"\step-probe-stderr.log"
MON_PORT = 14466
QEMU = r"C:\Program Files\qemu\qemu-system-x86_64"
EDK2 = ATTIC + r"\edk2-x86_64-code.fd"


def monitor_cmd(sock, cmd, wait=2.0):
    sock.sendall((cmd + "\n").encode())
    sock.settimeout(wait)
    buf = b""
    t0 = time.time()
    while time.time() - t0 < wait:
        try:
            chunk = sock.recv(65536)
            if not chunk:
                break
            buf += chunk
            if buf.rstrip().endswith(b"(qemu)"):
                break
        except socket.timeout:
            break
    return buf.decode("gbk", "replace")


def main():
    if os.path.exists(SERIAL):
        os.remove(SERIAL)
    proc = subprocess.Popen(
        [
            QEMU,
            "-M", "pc", "-m", "512M", "-smp", "2", "-cpu", "max",
            "-drive", f"if=pflash,format=raw,file={EDK2},unit=0",
            "-drive", f"file={DISK},format=vpc,if=ide,index=0",
            "-boot", "order=c", "-serial", f"file:{SERIAL}",
            "-display", "none", "-no-reboot", "-no-shutdown",
            "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait",
        ],
        stdout=open(ERRLOG, "wb"), stderr=subprocess.STDOUT,
    )
    print(f"[step] pid={proc.pid}，等待 AP 进内核（smp 前卡点 + 余量）…")
    deadline = time.time() + 180
    while time.time() < deadline:
        time.sleep(3)
        if proc.poll() is not None:
            print("[step] QEMU 提前退出")
            return
        if os.path.exists(SERIAL):
            t = open(SERIAL, "r", errors="replace").read()
            if "clock: source=tsc" in t:
                break
    time.sleep(14)  # 等 SIPI 与 AP 进内核（delay_10ms 较慢，给足余量）
    s = socket.create_connection(("127.0.0.1", MON_PORT), timeout=5)
    s.settimeout(2)
    try:
        s.recv(65536)
    except OSError:
        pass
    for c in ["stop", "cpu 1", "info registers", "x/2gx 0xffffffff8044f030",
              "x/4gx 0xffffffff80023790"]:
        print(f"---- {c} ----")
        print(monitor_cmd(s, c)[:3000])
    for i in range(6):
        print(f"---- stepi #{i+1} ----")
        print(monitor_cmd(s, "stepi")[:800])
        r = monitor_cmd(s, "info registers")[:900]
        for line in r.splitlines():
            if line.startswith("RIP=") or "RFL" in line:
                print("   ", line)
                break
    monitor_cmd(s, "quit", wait=1.5)
    try:
        proc.wait(timeout=6)
    except subprocess.TimeoutExpired:
        proc.kill()
    print("[step] done")


if __name__ == "__main__":
    main()
