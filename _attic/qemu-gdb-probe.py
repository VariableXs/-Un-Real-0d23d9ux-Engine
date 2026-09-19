#!/usr/bin/env python3
r"""GDB-RSP 单步取证：QEMU -gdb 模式下对 CPU#1 逐条单步，抓第一条不放行的指令。"""
import os
import socket
import subprocess
import time

ATTIC = os.path.dirname(os.path.abspath(__file__))
DISK = ATTIC + r"\uefi-esp.vhd"
SERIAL = ATTIC + r"\gdb-probe-serial.log"
ERRLOG = ATTIC + r"\gdb-probe-stderr.log"
GDB_PORT = 1234
QEMU = r"C:\Program Files\qemu\qemu-system-x86_64"
EDK2 = ATTIC + r"\edk2-x86_64-code.fd"


class Rsp:
    def __init__(self, port):
        self.s = socket.create_connection(("127.0.0.1", port), timeout=5)
        self.s.settimeout(4)
        self.seq = 0

    def _send(self, data: str):
        cks = sum(data.encode()) & 0xFF
        pkt = f"${data}#{cks:02x}".encode()
        self.s.sendall(pkt)
        ack = self.s.recv(1)
        while ack != b"+":
            ack = self.s.recv(1)

    def _recv(self) -> str:
        buf = b""
        while not buf.endswith(b"#"):
            buf += self.s.recv(4096)
        cks = self.s.recv(2)
        self.s.sendall(b"+")
        return buf[1:].decode("gbk", "replace")

    def cmd(self, data: str) -> str:
        self._send(data)
        return self._recv()

    def step(self):
        self.cmd("s")

    def regs(self) -> str:
        return self.cmd("g")

    def rip(self) -> int:
        r = self.regs()
        # x86-64: rax..r15 (8B each, LE), rip at offset 16*8
        hexrip = r[16 * 8:16 * 8 + 8]
        return int.from_bytes(bytes.fromhex(hexrip), "little")

    def read(self, addr: int, n: int) -> str:
        return self.cmd(f"m{addr:x},{n:x}")


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
            "-gdb", f"tcp::{GDB_PORT}",
            "-S",
        ],
        stdout=open(ERRLOG, "wb"), stderr=subprocess.STDOUT,
    )
    print(f"[gdb] pid={proc.pid} (-S 挂起启动 + gdbserver:{GDB_PORT})")
    time.sleep(2)
    g = Rsp(GDB_PORT)
    # 恢复运行（c 不回复，裸发送），等 AP 进内核
    g._send("c")
    print("[gdb] 运行中，等 clock 行…")
    deadline = time.time() + 180
    while time.time() < deadline:
        time.sleep(3)
        if os.path.exists(SERIAL):
            t = open(SERIAL, "r", errors="replace").read()
            if "clock: source=tsc" in t:
                break
    time.sleep(14)
    # 中断（Ctrl-C = 0x03）并切到 CPU#1
    g.s.sendall(b"\x03")
    time.sleep(1)
    try:
        stop = g._recv()
        print("[gdb] stop-reply:", stop[:80])
    except OSError:
        pass
    # gdb RSP 线程切换：Hc1 / vCont? 简化：用 'Hg1' 选线程 2（CPU#1）
    print("[gdb] Hg2 →", g.cmd("Hg2")[:40])
    print("[gdb] Hc2 →", g.cmd("Hc2")[:40])
    for i in range(12):
        try:
            g.step()
        except OSError as e:
            print(f"[gdb] step#{i} 连接中断: {e}")
            break
        rip = g.rip()
        print(f"step#{i}: RIP=0x{rip:x}")
        if i >= 3 and rip == 0xffffffff80023790:
            print("  → 卡在入口")
    # 若还在入口，读 CR2/CR3（x86 gdb: 0x1a=cr2? 直接试探）
    for reg in ("p1a", "p19", "p1b"):
        print(f"[gdb] {reg} →", g.cmd(reg)[:40])
    proc.kill()
    print("[gdb] done")


if __name__ == "__main__":
    main()
