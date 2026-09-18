#!/usr/bin/env python3
"""任务41+48（AI-B）· 实机联合探针驱动。

notepad：ring3 记事本闭环（4 DLL 16 导入绑定 → 消息泵全链 → 保存内容
         = 打开内容+注入字符 → VERDICT）。
picflow：画面流通道全链（模拟 VM 帧源 → 通道 → Limine 显存直写 → 上屏）
         + screendump 截图证据（渐变棋盘 + 窗口级红框可见）。

用法：python _attic/p41-p48-demo.py
证据：_attic/p41p48-serial.log + _attic/p41p48-screen1.png（画面流上屏）
"""
import os
import subprocess
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ATTIC = os.path.join(ROOT, "_attic")
ISO = os.path.join(ROOT, "varix-qemu.iso")
INIT_IMG = os.path.join(ATTIC, "p65-testdisk.img")
SERIAL = os.path.join(ATTIC, "p41p48-serial.log")
MON_PORT = 14662
SCREEN = os.path.join(ATTIC, "p41p48-screen1.png")
SCREEN2 = os.path.join(ATTIC, "p41p48-screen2.png")

TIMEOUT_S = 900
MARKERS_OK = "notepad: open→edit→save closed loop VERDICT=PASS"
MARKERS_BAD = "VERDICT=FAIL"


def read_log():
    try:
        with open(SERIAL, "r", errors="replace") as f:
            return f.read()
    except OSError:
        return ""


def make_disk():
    with open(INIT_IMG, "wb") as f:
        f.truncate(1_048_576 * 512)
        f.seek(60_000 * 512)
        f.write(b"P41P48-PROBE".ljust(512, b"0"))


def main():
    if not os.path.exists(ISO):
        print("missing ISO:", ISO)
        return 2
    make_disk()
    for p in (SERIAL, SCREEN, SCREEN2):
        if os.path.exists(p):
            os.remove(p)
    proc = subprocess.Popen(
        [
            "qemu-system-x86_64",
            "-cdrom", ISO,
            "-drive", f"file={INIT_IMG},if=none,id=nv1,format=raw",
            "-device", "nvme,drive=nv1,serial=P41P48",
            "-serial", "file:" + SERIAL,
            "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait",
            "-m", "1024",
            "-no-reboot", "-no-shutdown",
        ],
        cwd=ROOT,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    print("qemu pid", proc.pid, "— waiting for markers…")
    hit = False
    start = time.time()
    try:
        while time.time() - start < TIMEOUT_S:
            log = read_log()
            if MARKERS_OK in log or MARKERS_BAD in log:
                hit = True
                break
            time.sleep(2)
        # 收尾前抓两帧屏幕（画面流上屏证据：棋盘渐变+红框窗口帧可见）。
        try:
            import socket
            c = socket.create_connection(("127.0.0.1", MON_PORT), timeout=5)
            c.settimeout(5)
            buf = b""
            while b"(qemu)" not in buf:
                buf += c.recv(4096)
            c.sendall(b"screendump " + SCREEN.encode() + b"\n")
            out = b""
            while b"(qemu)" not in out:
                out += c.recv(65536)
            time.sleep(1)
            c.sendall(b"screendump " + SCREEN2.encode() + b"\n")
            out2 = b""
            while b"(qemu)" not in out2:
                out2 += c.recv(65536)
            c.close()
            print("screendump saved")
        except Exception as e:
            print("screendump failed:", e)
    finally:
        time.sleep(2)
        log = read_log()
        proc.terminate()
    # 证据行提取。
    keys = ["notepad:", "picflow-probe:", "vfs-probe:", "VERDICT"]
    lines = [ln for ln in log.splitlines() if any(k in ln for k in keys)]
    print("\n".join(lines[-25:]))
    print("---")
    print("RESULT:", "PASS" if MARKERS_OK in log else "FAIL")
    return 0 if MARKERS_OK in log else 1


if __name__ == "__main__":
    sys_exit = main()
    raise SystemExit(sys_exit)
