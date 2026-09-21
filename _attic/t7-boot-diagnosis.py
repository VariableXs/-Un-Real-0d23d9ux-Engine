#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""引导诊断：裸 ISO（无磁盘）vs -boot order=d，60s 内看串口是否有内核输出。"""
import os
import socket
import subprocess
import sys
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ATTIC = os.path.join(ROOT, "_attic")
ISO = os.path.join(ROOT, "varix-qemu.iso")
QEMU = "C:\\Program Files\\qemu\\qemu-system-x86_64.EXE"
SERIAL_PORT = 14738
MON_PORT = 14739


class Ser:
    def __init__(self, port):
        for _ in range(100):
            try:
                self.sock = socket.create_connection(("127.0.0.1", port), timeout=5)
                break
            except OSError:
                time.sleep(0.1)
        self.sock.settimeout(None)
        self.buf = b""
        self.alive = True
        self.t = threading.Thread(target=self._pump, daemon=True)
        self.t.start()

    def _pump(self):
        while self.alive:
            try:
                d = self.sock.recv(65536)
            except OSError:
                break
            if not d:
                break
            self.buf += d

    def text(self):
        return self.buf.decode(errors="replace")


def trial(name, extra, seconds=75):
    for p in (SERIAL_PORT, MON_PORT):
        wait_free(p)
    cmd = [
        QEMU, "-machine", "q35", "-m", "1024",
        "-cdrom", ISO,
        "-serial", f"tcp:127.0.0.1:{SERIAL_PORT},server,nowait",
        "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait",
    ] + extra
    proc = subprocess.Popen(cmd, cwd=ROOT)
    ser = Ser(SERIAL_PORT)
    t0 = time.time()
    time.sleep(seconds)
    txt = ser.text()
    print("== %s (%.0fs) ==" % (name, time.time() - t0))
    print("serial bytes:", len(ser.buf))
    tail = txt.strip().splitlines()
    for line in tail[-8:]:
        print("  |", line[:120])
    mon = Mon(MON_PORT)
    mon.cmd("quit")
    try:
        proc.wait(timeout=20)
    except subprocess.TimeoutExpired:
        proc.kill()
    ser.alive = False
    time.sleep(0.5)
    return len(ser.buf)


def wait_free(port):
    for _ in range(10):
        try:
            s = socket.create_connection(("127.0.0.1", port), timeout=0.3)
            s.close()
            time.sleep(2.0)
        except OSError:
            break


class Mon:
    def __init__(self, port):
        for _ in range(100):
            try:
                self.sock = socket.create_connection(("127.0.0.1", port), timeout=5)
                break
            except OSError:
                time.sleep(0.1)
        self.sock.settimeout(None)

    def cmd(self, c):
        self.sock.sendall((c + "\n").encode())
        time.sleep(0.3)


if __name__ == "__main__":
    n1 = trial("bare ISO", [])
    n2 = trial("ISO + boot order=d", ["-boot", "order=d"])
    print("VERDICT bare=%d bytes, order-d=%d bytes" % (n1, n2))
