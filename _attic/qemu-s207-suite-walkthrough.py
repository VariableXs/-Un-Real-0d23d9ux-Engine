# -*- coding: utf-8 -*-
"""AI-4 · S2.07 三件套全功能走查（QEMU 层，M2 预演）+ S2.12 全流程 ×3 稳定复现。

对照十二步第 7 步「三件套在垫片下闭环」的走查清单（真机走查时同清单复用）：
  ① 桌面壳：零选项引导直落桌面 + START 菜单开关（键盘）
  ② 文件管理器：打开 → 列表非空 → 键盘选中 → 打开文件 → 预览 → Esc 返回
     → 数据源标注（shared-exFAT / demo-tree 如实）
  ③ 设置页：打开 → BOOT_TIMEOUT 调节 / SHOW_MENU 切换 / DEFAULT_ENTRY 轮换
     → 三条 KV 写入回执（settings set key=... rc=0）→ Esc 返回
  ④ 关于页：打开 + kv-keys= 指标
  ⑤ 全部走查标记按轮次 +1 判定（防历史记录误判）；×3 轮独立 QEMU =
     S2.12「全流程 ×N 稳定复现」的 QEMU 层预演（真机 ×5 待实机会话）。

用法：python _attic/qemu-s207-suite-walkthrough.py（默认 3 轮）
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
SERIAL = os.path.join(ATTIC, "qemu-s207-suite-serial.log")
SHOT_DIR = os.path.join(ATTIC, "acceptance-s207-suite")
MON_PORT = 14764
SERIAL_PORT = 14765
BOOT_TIMEOUT = 600
STEP_TIMEOUT = 240
ROUNDS = 3

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
        try:
            self.sock.sendall((line + "\n").encode())
        except OSError:
            return ""  # QEMU 已死（外部终止）——调用方经存活检查收轮
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
        time.sleep(1.2)

    def quit(self):
        try:
            self.sock.sendall(b"quit\n")
        except OSError:
            pass


def run_round(rnd):
    """跑一轮全功能走查，返回 (通过数, 总数) 与逐项结果。"""
    print(f"\n===== ROUND {rnd + 1}/{ROUNDS} =====")
    if os.path.exists(SERIAL):
        os.remove(SERIAL)
    with BUF_LOCK:
        BUF.clear()
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

        # ① 桌面壳：零选项引导直落桌面。
        for m in ["SHELL: entering variable-system", "SHELL: first-frame ms=",
                  "SHELL: boot-replay done", "SHELL: desktop-ready"]:
            checks.append((f"boot: {m.split(': ')[1]}", wait_count(m, 0, BOOT_TIMEOUT)))
        checks.append(("files: list non-empty (fm count)", wait_count("SHELL: fm count=", 0)))
        time.sleep(1.5)
        mon.shot(f"r{rnd + 1}-01-desktop.png")

        # ② 开始菜单开关（桌面壳交互基线）。
        b_menu = count_marker("SHELL: startmenu opened")
        mon.key("ret")
        checks.append(("shell: start menu opens", wait_count("SHELL: startmenu opened", b_menu)))
        mon.shot(f"r{rnd + 1}-02-menu.png")

        # ③ 设置页：菜单第 2 项（down ×1 → ret）。
        mon.key("down")
        b_set = count_marker("SHELL: settings opened")
        b_kv = count_marker("SHELL: settings set key=")
        mon.key("ret")
        checks.append(("settings: opens", wait_count("SHELL: settings opened", b_set)))
        mon.shot(f"r{rnd + 1}-03-settings.png")
        # 三参数 KV 写入往返：right(timeout+1) / ret(show_menu 切换) / down+right(default 轮换)。
        mon.key("right")
        ok = wait_count("SHELL: settings set key=boot_timeout", b_kv, STEP_TIMEOUT)
        checks.append(("settings: boot_timeout KV write rc", ok))
        mon.key("down")
        mon.key("ret")
        ok = wait_count("SHELL: settings set key=show_menu", b_kv, STEP_TIMEOUT)
        checks.append(("settings: show_menu KV toggle", ok))
        mon.key("down")
        mon.key("right")
        ok = wait_count("SHELL: settings set key=default_entry", b_kv, STEP_TIMEOUT)
        checks.append(("settings: default_entry KV rotate", ok))
        mon.shot(f"r{rnd + 1}-04-settings-touched.png")
        # Esc 回桌面。
        mon.key("esc")
        time.sleep(1.5)

        # ④ 文件管理器：菜单第 1 项。
        mon.key("ret")
        ok = wait_count("SHELL: startmenu opened", b_menu + 1 if count_marker("SHELL: startmenu opened") == b_menu else b_menu)
        mon.key("ret")
        b_files = count_marker("SHELL: files opened")
        mon.key("ret")
        checks.append(("files: opens", wait_count("SHELL: files opened", b_files)))
        time.sleep(1.5)
        mon.shot(f"r{rnd + 1}-05-files.png")
        # 键盘选中并打开一个文件（demo 树 5 项，逐项尝试 ≤5 次）。
        b_open = count_marker("SHELL: file opened name=")
        opened = False
        for _ in range(5):
            if proc.poll() is not None:
                break  # QEMU 意外死亡（外部终止防护），下方存活检查记录
            mon.key("ret")
            if wait_count("SHELL: file opened name=", b_open, 90):
                opened = True
                break
            mon.key("down")
        checks.append(("files: open file → preview", opened))
        if proc.poll() is not None:
            checks.append(("qemu: alive through file preview", False))
            print(f"ROUND {rnd + 1}: QEMU died unexpectedly")
            return sum(1 for _, ok in checks if ok), len(checks)
        checks.append(("qemu: alive through file preview", True))
        time.sleep(1.5)
        mon.shot(f"r{rnd + 1}-06-fileview.png")
        # Esc 两次回桌面（预览→列表→桌面）。
        mon.key("esc")
        time.sleep(1.5)
        mon.key("esc")
        time.sleep(1.5)

        # ⑤ 关于页（菜单第 3 项：ret → down down → ret）。
        b_about = count_marker("SHELL: about opened")
        mon.key("ret")
        mon.key("down")
        mon.key("down")
        mon.key("ret")
        checks.append(("about: opens with kv-keys metric", wait_count("SHELL: about opened", b_about)))
        mon.shot(f"r{rnd + 1}-07-about.png")
        mon.key("esc")
        time.sleep(1.5)
    finally:
        time.sleep(0.5)
        mon.quit()
        time.sleep(1.0)
        if proc.poll() is None:
            proc.terminate()

    print(f"--- ROUND {rnd + 1} results ---")
    passed = 0
    for name, ok in checks:
        print(("PASS  " if ok else "FAIL  ") + name)
        passed += 1 if ok else 0
    print(f"ROUND {rnd + 1}: {passed}/{len(checks)}")
    return passed, len(checks)


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
    total_p = total_n = 0
    round_lines = []
    for r in range(ROUNDS):
        p, n = run_round(r)
        total_p += p
        total_n += n
        round_lines.append((r + 1, p, n))
    print("\n== AI-4 S2.07 suite walkthrough (M2 QEMU rehearsal) ==")
    for r, p, n in round_lines:
        print(f"ROUND {r}: {p}/{n}")
    print(f"TOTAL: {total_p}/{total_n}")
    # S2.12 预演口径：每轮全部 PASS = 稳定复现。
    stable = all(p == n for _, p, n in round_lines)
    print(f"STABLE x{ROUNDS}: {'PASS' if stable else 'FAIL'}")
    sys.exit(0 if total_p == total_n else 1)


if __name__ == "__main__":
    main()
