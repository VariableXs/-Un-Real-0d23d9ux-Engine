#!/usr/bin/env python3
"""S0.2 boot-select.json 配置桥 QEMU 全链演练（AI-1 · 2026-09-21）。

三变体 + 防自锁闸门，全部以串口标记判定（+1 基线纪律沿用
qemu-shell-walkthrough.py 范式；串口走 -serial tcp 实时流）：

  v1 present  isoroot/boot-select.json = {"timeout_sec": 2}
      → boot-diag timeout=2（Limine internal-module 通道生效，2≠默认5 是硬证据）
      → 倒计时走默认 A 卡 → 防自锁闸门拒绝（QEMU 无 Windows 引导项可证实）
      → 如实落 ushell（boot completed）
  v2 corrupt  isoroot/boot-select.json = 半截 JSON
      → kwarn "shared config corrupt" → 内置默认 timeout=5 → 引导不炸
  v3 absent   isoroot 不放配置（make-iso --no-seed）
      → 无 corrupt 告警、静默内置默认 timeout=5 → 引导不破（副本缺失=契约不破）

内核 ELF 用 worktree（S0.1 提交态）kbuild 产物；收尾走 monitor `quit`
（本演练只看引导期语义，不测 S5 关机）。产物：_attic/bootcfg-walk-serial-<v>.log。
"""

import os
import shutil
import socket
import subprocess
import sys
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))      # 主仓
WT = os.environ.get("VARIX_WALK_WT", r"D:\2\14\varix-wt-s01")           # worktree（S0.1 提交态）
ATTIC = os.path.join(ROOT, "_attic")
SERIAL_PORT = 14756
MON_PORT = 14757
STEP_TIMEOUT = 180


def sh(cmd, cwd=None):
    print("+", " ".join(cmd))
    return subprocess.run(cmd, cwd=cwd or ROOT, capture_output=True, text=True)


def prepare_isoroot():
    """worktree 没有 gitignored 的 build/ 产物：手动备齐 isoroot 骨架。"""
    isoroot = os.path.join(WT, "build", "isoroot")
    os.makedirs(os.path.join(isoroot, "kernel"), exist_ok=True)
    dst_conf = os.path.join(isoroot, "limine.conf")
    if not os.path.isfile(dst_conf):
        shutil.copy2(os.path.join(WT, "limine.conf"), dst_conf)
    dst_initrd = os.path.join(isoroot, "initrd.img")
    if not os.path.isfile(dst_initrd):
        shutil.copy2(os.path.join(ROOT, "build", "initrd.img"), dst_initrd)
    return isoroot


def build_iso(name, no_seed=False):
    isoroot = prepare_isoroot()
    cfg = os.path.join(isoroot, "boot-select.json")
    if name == "present":
        with open(cfg, "w", encoding="utf-8", newline="\n") as f:
            f.write('{"timeout_sec": 2}\n')
    elif name == "corrupt":
        with open(cfg, "w", encoding="utf-8", newline="\n") as f:
            f.write('{"timeout_sec": ')  # 半截 JSON：容错第 3 层应整体重置
    else:  # absent
        if os.path.isfile(cfg):
            os.remove(cfg)
    out = os.path.join(ATTIC, f"bootcfg-{name}.iso")
    args = [sys.executable, os.path.join(WT, "scripts", "make-iso-qemu.py"),
            "--out", out]
    if no_seed:
        args.append("--no-seed")
    r = sh(args, cwd=WT)
    if r.returncode != 0:
        print(r.stdout, r.stderr)
        raise SystemExit("make-iso failed: " + name)
    return out


class SerialTee:
    """-serial tcp 实时流 → 内存缓冲（判定）+ 证据文件（落盘）。"""

    def __init__(self, port, path):
        self.lock = threading.Lock()
        self.buf = b""
        self.alive = True
        self.f = open(path, "wb")
        self.sock = self._connect(port)
        self.sock.settimeout(None)  # 超时传染戒律：recv 不得继承 connect 超时
        threading.Thread(target=self._pump, daemon=True).start()

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
                data = self.sock.recv(65536)
            except OSError:
                break
            if not data:
                break
            with self.lock:
                self.buf += data
            self.f.write(data)
            self.f.flush()

    def count(self, marker):
        with self.lock:
            return self.buf.count(marker.encode())

    def close(self):
        self.alive = False
        try:
            self.sock.close()
        except Exception:
            pass
        self.f.close()


class Mon:
    def __init__(self, port):
        self.s = socket.create_connection(("127.0.0.1", port), timeout=10)
        time.sleep(0.5)
        self.s.recv(65536)

    def cmd(self, c):
        try:
            self.s.sendall(c.encode() + b"\n")
        except OSError:
            return ""
        time.sleep(0.6)
        try:
            return self.s.recv(65536).decode(errors="replace")
        except Exception:
            return ""


def wait_count(serial, marker, before, timeout=STEP_TIMEOUT):
    t0 = time.time()
    while time.time() - t0 < timeout:
        if serial.count(marker) > before:
            print(f"  [{time.time()-t0:6.1f}s] MARKER: {marker}")
            return True
        time.sleep(0.5)
    print(f"  [{time.time()-t0:6.1f}s] MARKER TIMEOUT: {marker} (serial {len(serial.buf)}B)")
    return False


def run_variant(name, iso, checks):
    """checks: [(label, marker_or_None, expect)]，None=负向断言（演练结束时核对）。"""
    log = os.path.join(ATTIC, f"bootcfg-walk-serial-{name}.log")
    if os.path.exists(log):
        os.remove(log)
    proc = subprocess.Popen(
        [
            "qemu-system-x86_64",
            "-m", "1024",
            "-cdrom", iso,
            "-display", "none",
            "-no-reboot",
            "-serial", f"tcp:127.0.0.1:{SERIAL_PORT},server,nowait",
            "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait",
        ],
        cwd=ROOT,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    results = []
    serial = None
    try:
        serial = SerialTee(SERIAL_PORT, log)
        mon = Mon(MON_PORT)
        for label, marker, expect in checks:
            if marker is None:
                continue
            before = serial.count(marker)
            ok = wait_count(serial, marker, before)
            results.append((label, ok == expect))
        time.sleep(2)
        mon.cmd("quit")
    finally:
        try:
            proc.wait(timeout=30)
        except subprocess.TimeoutExpired:
            proc.kill()
        if serial:
            serial.close()
    # 负向断言：全程缓冲里不许出现
    for label, marker, expect in checks:
        if marker is None:
            continue
        if not expect:
            results.append((label + " (negative)", serial.count(marker) == 0))
    return results


def main():
    all_ok = True
    plan = [
        ("present", build_iso("present"), [
            ("模块通道生效 timeout=2（≠默认5）", "timeout=2 default=varix", True),
            ("防自锁闸门拒绝盲写 BootNext", "refusing to guess", True),
            ("如实落 ushell（引导不炸）", "boot completed", True),
        ]),
        ("corrupt", build_iso("corrupt"), [
            ("整体损坏如实上报 kwarn", "shared config corrupt", True),
            ("损坏回落内置默认 timeout=5", "timeout=5 default=varix", True),
            ("容错第三层：引导不炸", "boot completed", True),
        ]),
        ("absent", build_iso("absent", no_seed=True), [
            ("副本缺失静默内置默认 timeout=5", "timeout=5 default=varix", True),
            ("副本缺失：无损坏告警", "shared config corrupt", False),
            ("契约不破：引导不炸", "boot completed", True),
        ]),
    ]
    for name, iso, checks in plan:
        print(f"\n===== variant: {name} =====")
        results = run_variant(name, iso, checks)
        for label, ok in results:
            print(f"  {label}: {'PASS' if ok else 'FAIL'}")
            all_ok &= ok
    print("\nBOOTCFG WALKTHROUGH VERDICT:", "PASS" if all_ok else "FAIL")
    return 0 if all_ok else 1


if __name__ == "__main__":
    sys.exit(main())
