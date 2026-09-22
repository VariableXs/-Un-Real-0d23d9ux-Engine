#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""实机 win-probe 卡死复现实验（2026-09-22）。

背景：真机（1920×1080，DisplayService=Direct 直写模式）在
`win-probe: screen 1920x1080` 之后静默卡死；QEMU（1280×800，双缓冲模式）
从不复现。两世界唯一结构性差异 = 屏幕分辨率 >4MiB 与否决定显示服务模式，
win_probe 的直写合成路径 + 首个 order-10 PMM 分配只在真机被走到。

本实验把 QEMU 强制到 1920×1080（limine.conf framebuffer_* 键），
若 QEMU 复现同一卡点 → 真机 bug 本地化，可串口级调试。

用法：python _attic/repro-1080p-winprobe.py [--timeout 300]
"""
import os
import re
import shutil
import socket
import subprocess
import sys
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ATTIC = os.path.join(ROOT, "_attic")
PY = r"C:\Users\varia\.workbuddy\binaries\python\envs\default\Scripts\python.exe"
MAKE_ISO = os.path.join(ROOT, "scripts", "make-iso-qemu.py")
REPRO_ISO = os.path.join(ATTIC, "varix-1080p.iso")
REPRO_CONF = os.path.join(ATTIC, "limine-1080p.conf")
SERIAL_LOG = os.path.join(ATTIC, "repro-1080p-serial.log")
SHOT = os.path.join(ATTIC, "repro-1080p-screen.png")
TEST_IMG = os.path.join(ATTIC, "t7-testdisk.img")
SHARED_IMG = os.path.join(ATTIC, "t7-shared-exfat.img")
MKEXFAT = os.path.join(ATTIC, "mkexfat.py")
QEMU = "C:\\Program Files\\qemu\\qemu-system-x86_64.EXE"
SERIAL_PORT = 14736
MON_PORT = 14737

CONF_BODY = """timeout: 0
serial: yes

/kernel/varix
    protocol: limine
    kernel_path: boot():/kernel/varix
    framebuffer_width: 1920
    framebuffer_height: 1080
"""


def sh(cmd):
    print("+", " ".join(cmd))
    return subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)


def kill_orphan_qemu():
    r = subprocess.run(["tasklist"], capture_output=True, text=True)
    if "qemu-system" in (r.stdout or "").lower():
        print("[setup] 发现孤儿 QEMU，按进程名强杀（战役戒律）")
        subprocess.run(["taskkill", "/F", "/IM", "qemu-system-x86_64.EXE"],
                       capture_output=True)


def wait_port_free(port):
    for _ in range(10):
        try:
            probe = socket.create_connection(("127.0.0.1", port), timeout=0.3)
            probe.close()
            time.sleep(2.0)
        except OSError:
            return
    raise SystemExit("port %d still busy" % port)


def ensure_disks():
    if not os.path.isfile(TEST_IMG):
        with open(TEST_IMG, "wb") as f:
            f.truncate(2_000_000 * 512)
            f.seek(1_500_000 * 512)
            f.write(b"T7-PROBE".ljust(512, b"0"))
        print("[setup] test disk created")
    if not os.path.isfile(SHARED_IMG):
        r = sh([PY, MKEXFAT, SHARED_IMG])
        if r.returncode != 0:
            raise SystemExit("mkexfat failed: " + r.stderr[-400:])
        print("[setup] shared exfat img created")


def build_iso():
    with open(REPRO_CONF, "w", encoding="utf-8", newline="\n") as f:
        f.write(CONF_BODY)
    r = sh([PY, MAKE_ISO, "--conf", REPRO_CONF, "--out", REPRO_ISO])
    if r.returncode != 0:
        print(r.stdout[-800:])
        print(r.stderr[-800:])
        raise SystemExit("make-iso failed")
    print("[setup] repro ISO built: %s (%d bytes)" % (REPRO_ISO, os.path.getsize(REPRO_ISO)))


class SerialTee:
    def __init__(self, port, path):
        self.lock = threading.Lock()
        self.buf = b""
        self.alive = True
        self.f = open(path, "wb")
        self.sock = self._connect(port)
        self.sock.settimeout(None)
        self.t = threading.Thread(target=self._pump, daemon=True)
        self.t.start()

    def _connect(self, port):
        last = None
        for _ in range(200):
            try:
                return socket.create_connection(("127.0.0.1", port), timeout=5)
            except OSError as e:
                last = e
                time.sleep(0.1)
        raise SystemExit("serial connect failed: " + str(last))

    def _pump(self):
        while self.alive:
            try:
                d = self.sock.recv(65536)
            except OSError:
                break
            if not d:
                break
            with self.lock:
                self.buf += d
            self.f.write(d)
            self.f.flush()

    def count(self, marker):
        with self.lock:
            return self.buf.count(marker.encode())

    def tail(self, n=1800):
        with self.lock:
            return self.buf[-n:].decode(errors="replace")

    def close(self):
        self.alive = False
        try:
            self.sock.close()
        except OSError:
            pass
        self.f.close()


class Mon:
    def __init__(self, port):
        for _ in range(200):
            try:
                self.sock = socket.create_connection(("127.0.0.1", port), timeout=5)
                break
            except OSError:
                time.sleep(0.1)
        else:
            raise SystemExit("monitor connect failed")
        self.sock.settimeout(None)

    def shot(self, path):
        try:
            self.sock.sendall(("screendump %s\n" % path).encode())
            time.sleep(0.5)
        except OSError as e:
            print("  [warn] screendump failed: %s" % e)

    def quit(self):
        try:
            self.sock.sendall(b"quit\n")
        except OSError:
            pass


def main():
    timeout = 300
    smp = 1
    tag = "1080p"
    args = sys.argv[1:]
    if "--timeout" in args:
        timeout = int(args[args.index("--timeout") + 1])
    if "--smp" in args:
        smp = int(args[args.index("--smp") + 1])
        tag += "-smp%d" % smp
    serial_log = os.path.join(ATTIC, "repro-%s-serial.log" % tag)

    kill_orphan_qemu()
    wait_port_free(SERIAL_PORT)
    wait_port_free(MON_PORT)
    ensure_disks()
    build_iso()

    if os.path.isfile(serial_log):
        os.remove(serial_log)

    cmd = [
        QEMU, "-machine", "q35", "-m", "1024",
        "-smp", str(smp),
        "-boot", "order=d",
        "-device", "VGA,edid=on,xres=1920,yres=1080",
        "-cdrom", REPRO_ISO,
        "-drive", "file=%s,if=none,id=nv1,format=raw" % TEST_IMG,
        "-device", "nvme,drive=nv1,serial=T7TEST",
        "-drive", "file=%s,if=none,id=nv2,format=raw" % SHARED_IMG,
        "-device", "nvme,drive=nv2,serial=T7SHAR",
        "-serial", "tcp:127.0.0.1:%d,server,nowait" % SERIAL_PORT,
        "-monitor", "tcp:127.0.0.1:%d,server,nowait" % MON_PORT,
    ]
    print("+", " ".join(cmd))
    proc = subprocess.Popen(cmd, cwd=ROOT)
    ser = SerialTee(SERIAL_PORT, serial_log)
    mon = Mon(MON_PORT)

    marks = {
        "reach-1080p(win-probe: screen 1920x1080)": "win-probe: screen 1920x1080",
        "win-probe PASS(未复现)": "win-probe: PASS",
        "desktop-ready(全链通过)": "SHELL: desktop-ready",
    }
    hit = {}
    deadline = time.time() + timeout
    while time.time() < deadline:
        for k, m in marks.items():
            if k not in hit and ser.count(m) > 0:
                hit[k] = time.time()
                print("  [mark] %s  (+%.1fs)" % (k, time.time() - (deadline - timeout)))
        if "desktop-ready(全链通过)" in hit:
            break
        rc = proc.poll()
        if rc is not None:
            print("  [qemu exited rc=%s]" % rc)
            break
        time.sleep(1.0)

    mon.shot(SHOT)
    time.sleep(0.5)
    mon.quit()
    try:
        rc = proc.wait(timeout=15)
    except subprocess.TimeoutExpired:
        proc.kill()
        rc = -1
    ser.close()

    print("=" * 64)
    print("RESULT:")
    for k in marks:
        print("  %-46s %s" % (k, "HIT" if k in hit else "miss"))
    tail = ser.tail()
    if "win-probe: PASS" not in tail and "desktop-ready" not in tail:
        print("-" * 64)
        print("SERIAL TAIL (last lines):")
        print(tail)
    print("screendump: %s" % SHOT)
    print("serial log: %s" % serial_log)
    reproduced = ("reach-1080p(win-probe: screen 1920x1080)" in hit
                  and "win-probe PASS(未复现)" not in hit)
    print("=" * 64)
    print("REPRODUCED: %s" % ("YES — QEMU 卡在与真机相同的 win-probe 卡点" if reproduced
                             else "NO — 1080p 下 QEMU 未复现（差异在硬件侧）"))
    return 0 if reproduced else 1


if __name__ == "__main__":
    sys.exit(main())
