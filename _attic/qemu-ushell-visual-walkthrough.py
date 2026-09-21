# -*- coding: utf-8 -*-
"""AI-4 · ushell 视觉对齐 + 鼠标通道 QEMU 走查（复用 winsurf 走查范式）。

验证链：
  A. 引导标记链零回归：entering → first-frame → boot-replay → desktop-ready → fm count
  B. 视觉截图：01-loading / 02-desktop（光标在屏幕中心）/ 04-menu / 05-files
  C. 鼠标通道（S2.05 R2 消费端闭环证据）：HMP mouse_move+mouse_button 点击
     START → 「SHELL: startmenu opened」（鼠标打开菜单=既有标记复用）
  D. 键盘回归：sendkey ret 开菜单 → Esc（键盘路径不受鼠标改造影响）
用法：python _attic/qemu-ushell-visual-walkthrough.py
"""
import os
import re
import socket
import subprocess
import sys
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ATTIC = os.path.join(ROOT, "_attic")
ISO = os.path.join(ROOT, "varix-qemu.iso")
SERIAL = os.path.join(ATTIC, "qemu-ushell-visual-serial.log")
SHOT_DIR = os.path.join(ATTIC, "acceptance-ushell-visual")
MON_PORT = 14754
SERIAL_PORT = 14755
BOOT_TIMEOUT = 600
STEP_TIMEOUT = 240

os.makedirs(SHOT_DIR, exist_ok=True)
BUF_LOCK = threading.Lock()
BUF = []


def _connect_retry(port, tries=15, delay=1.0):
    last = None
    for _ in range(tries):
        try:
            return socket.create_connection(("127.0.0.1", port), timeout=2.0)
        except OSError as e:
            last = e
            time.sleep(delay)
    raise last


class SerialTee:
    def __init__(self, port, logfile):
        self.sock = _connect_retry(port)
        self.sock.settimeout(None)
        self.log = open(logfile, "ab", buffering=0)
        self.thread = threading.Thread(target=self._pump, daemon=True)
        self.thread.start()

    def _pump(self):
        while True:
            try:
                data = self.sock.recv(4096)
            except OSError:
                break
            if not data:
                break
            with BUF_LOCK:
                BUF.append(data)
            self.log.write(data)


def serial_text():
    with BUF_LOCK:
        return b"".join(BUF).decode("utf-8", "replace")


def count_marker(marker):
    return serial_text().count(marker)


def wait_count(marker, before, timeout=STEP_TIMEOUT):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if count_marker(marker) > before:
            return True
        time.sleep(1.0)
    return False


class Mon:
    def __init__(self, port):
        self.sock = _connect_retry(port)
        self.sock.settimeout(None)
        time.sleep(0.3)
        try:
            self.sock.recv(65536)
        except OSError:
            pass

    def cmd(self, line):
        self.sock.sendall((line + "\n").encode())
        time.sleep(0.4)
        try:
            return self.sock.recv(65536).decode("utf-8", "replace")
        except OSError:
            return ""

    def shot(self, name):
        self.cmd(f"screendump {os.path.join(SHOT_DIR, name)}")
        print(f"  [shot] {name}")

    def key(self, k):
        self.cmd(f"sendkey {k}")

    def mouse_move(self, dx, dy):
        self.cmd(f"mouse_move {dx} {dy}")

    def mouse_button(self, state):
        self.cmd(f"mouse_button {state}")

    def quit(self):
        try:
            self.sock.sendall(b"quit\n")
        except OSError:
            pass


def main():
    if not os.path.exists(ISO):
        raise SystemExit("missing ISO: " + ISO)
    for _ in range(10):
        try:
            probe = socket.create_connection(("127.0.0.1", SERIAL_PORT), timeout=0.3)
            probe.close()
            print(f"  port {SERIAL_PORT} busy — waiting...")
            time.sleep(2.0)
        except OSError:
            break
    if os.path.exists(SERIAL):
        os.remove(SERIAL)
    proc = subprocess.Popen(
        [
            "qemu-system-x86_64",
            "-machine", "q35",
            "-cdrom", ISO,
            "-serial", f"tcp:127.0.0.1:{SERIAL_PORT},server,nowait",
            "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait",
            "-m", "1024",
        ],
        cwd=ROOT,
    )
    SerialTee(SERIAL_PORT, SERIAL)
    checks = []
    try:
        mon = Mon(MON_PORT)

        # A. 引导标记链（零输入直落桌面）。
        ok = wait_count("SHELL: entering variable-system", 0)
        checks.append(("A1 entering marker", ok))
        ok = wait_count("SHELL: first-frame ms=", 0)
        checks.append(("A2 first-frame marker", ok))
        mon.shot("01-loading.png")
        ok = wait_count("SHELL: boot-replay done", 0)
        checks.append(("A3 boot-replay done", ok))
        ok = wait_count("SHELL: desktop-ready", 0)
        checks.append(("A4 desktop-ready", ok))
        ok = wait_count("SHELL: fm count=", 0)
        checks.append(("A5 fm count reported", ok))
        time.sleep(1.5)
        mon.shot("02-desktop.png")

        # C. 鼠标通道：光标初始在屏幕中心（约 640,400 @1280x800）。
        #    移到 START 按钮（约 60,768）：分批相对位移（单包 ±255）。
        for dx, dy in [(-200, 120), (-200, 120), (-180, 130)]:
            mon.mouse_move(dx, dy)
            time.sleep(2.0)
        mon.shot("03-start-hover.png")
        before_menu = count_marker("SHELL: startmenu opened")
        mon.mouse_button(1)   # 左键按下
        time.sleep(1.5)
        mon.mouse_button(0)   # 释放
        ok = wait_count("SHELL: startmenu opened", before_menu)
        checks.append(("C1 mouse opens start menu (click START)", ok))
        time.sleep(2.0)
        mon.shot("04-menu.png")

        # D. 键盘回归：Esc 关菜单 → ret 重开（键盘路径不受影响）。
        mon.key("esc")
        time.sleep(2.0)
        mon.key("ret")
        ok = wait_count("SHELL: startmenu opened", before_menu + 1)
        checks.append(("D1 keyboard reopens menu (regression)", ok))
        time.sleep(2.0)

        # C2. 鼠标点击菜单 Files 项（第一项，几何 8+14..300, 顶部 32+0..48）。
        for dx, dy in [(-20, -660), (10, -60)]:
            mon.mouse_move(dx, dy)
            time.sleep(2.0)
        before_files = count_marker("SHELL: files opened")
        mon.mouse_button(1)
        time.sleep(1.5)
        mon.mouse_button(0)
        ok = wait_count("SHELL: files opened", before_files)
        checks.append(("C2 mouse opens Files page", ok))
        time.sleep(2.0)
        mon.shot("05-files.png")

        # E. 键盘 Esc 回桌面（回归）。
        mon.key("esc")
        time.sleep(2.0)
        mon.shot("06-back-desktop.png")
    finally:
        time.sleep(0.5)
        mon.quit()
        time.sleep(1.0)
        if proc.poll() is None:
            proc.terminate()

    print("\n== AI-4 ushell visual/mouse walkthrough ==")
    failed = 0
    for name, ok in checks:
        print(("PASS  " if ok else "FAIL  ") + name)
        failed += 0 if ok else 1
    print(f"\n{len(checks) - failed}/{len(checks)} PASS")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
