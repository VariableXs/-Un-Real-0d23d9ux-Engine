#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""storage_selftest 门禁双态验证（2026-09-22 引导设施红线修复）。

背景：nvme::probe_and_selftest 历史上对 hits[0]（真机 = 内置系统 NVMe）
无条件跑六连直写探针，写坏 GPT 表项数组与 ESP —— 「BIOS 引导程序丢失 +
DiskGenius GPT CRC 错误」的根因。修复：直写探针全部锁进 cmdline 门禁
`storage_selftest=1`（与 ahci_selftest 同范式），默认路径零写入。

本脚本在 QEMU（双 NVMe：t7-testdisk 刮擦盘 + t7-shared-exfat）验证两态：
  off 态（repo 根 limine.conf，无 cmdline）：
    必须出现  "nvme: write probes SKIPPED (storage_selftest off ...)"
    绝不出现 "nvme: loopback"（一次都不许写）
  on 态（_attic/limine-qemu-selftest.conf）：
    必须出现  "nvme: storage_selftest=1"
    必须出现  "nvme: loopback x1000 passed=true"
    必须出现  fs23-disk / milestone / kv 存储服务 / vfsguard / kvault 探针日志

用法：python _attic/verify-storage-gate.py [--timeout 420]
"""
import os
import subprocess
import sys
import threading
import time
import socket

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ATTIC = os.path.join(ROOT, "_attic")
PY = r"C:\Users\varia\.workbuddy\binaries\python\envs\default\Scripts\python.exe"
MAKE_ISO = os.path.join(ROOT, "scripts", "make-iso-qemu.py")
CONF_ROOT = os.path.join(ROOT, "limine.conf")
CONF_SELFTEST = os.path.join(ATTIC, "limine-qemu-selftest.conf")
TEST_IMG = os.path.join(ATTIC, "t7-testdisk.img")
SHARED_IMG = os.path.join(ATTIC, "t7-shared-exfat.img")
MKEXFAT = os.path.join(ATTIC, "mkexfat.py")
QEMU = "C:\\Program Files\\qemu\\qemu-system-x86_64.EXE"
SERIAL_PORT = 14736
MON_PORT = 14737


def sh(cmd):
    print("+", " ".join(cmd))
    # errors="replace"：make-iso 输出含 GBK 中文，严格 utf-8 解码会让
    # subprocess 读线程抛 UnicodeDecodeError（08:5x 实测）。
    return subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True,
                          errors="replace")


def kill_orphan_qemu():
    # errors="replace"：tasklist 输出 GBK 中文，严格 utf-8 会让读线程崩溃。
    r = subprocess.run(["tasklist"], capture_output=True, text=True,
                       errors="replace")
    if "qemu-system" in (r.stdout or "").lower():
        print("[setup] 孤儿 QEMU，按进程名强杀")
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


def build_iso(conf, out):
    r = sh([PY, MAKE_ISO, "--conf", conf, "--out", out])
    if r.returncode != 0:
        print(r.stdout[-800:])
        print(r.stderr[-800:])
        raise SystemExit("make-iso failed: %s" % out)
    print("[setup] ISO built: %s (%d bytes)" % (out, os.path.getsize(out)))


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

    def tail(self, n=1500):
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

    def quit(self):
        try:
            self.sock.sendall(b"quit\n")
        except OSError:
            pass


def run_case(tag, conf_path, iso_path, timeout):
    """跑一态，返回 (必须出现标记集, 绝不出现标记集, 串口日志路径)。"""
    serial_log = os.path.join(ATTIC, "verify-gate-%s-serial.log" % tag)
    build_iso(conf_path, iso_path)
    if os.path.isfile(serial_log):
        os.remove(serial_log)

    cmd = [
        QEMU, "-machine", "q35", "-m", "1024",
        "-smp", "1",
        "-boot", "order=d",
        "-device", "VGA,edid=on",
        "-cdrom", iso_path,
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

    # 两态共用的「引导到达」标记：出现即说明内核完整走完存储探针区。
    reached = "SHELL: desktop-ready"
    deadline = time.time() + timeout
    start = time.time()
    while time.time() < deadline:
        if ser.count(reached) > 0:
            print("  [%s] boot reached desktop-ready (+%.1fs)" % (tag, time.time() - start))
            time.sleep(2.0)  # 留缓冲让尾部日志进 tee
            break
        rc = proc.poll()
        if rc is not None:
            print("  [%s] qemu exited rc=%s (+%.1fs)" % (tag, rc, time.time() - start))
            break
        time.sleep(1.0)
    else:
        print("  [%s] TIMEOUT %ds" % (tag, timeout))

    mon.quit()
    try:
        proc.wait(timeout=15)
    except subprocess.TimeoutExpired:
        proc.kill()
    ser.close()
    return serial_log


def grep_log(path, marker):
    try:
        with open(path, "rb") as f:
            return f.read().count(marker.encode())
    except OSError:
        return 0


def main():
    timeout = 420
    args = sys.argv[1:]
    if "--timeout" in args:
        timeout = int(args[args.index("--timeout") + 1])

    kill_orphan_qemu()
    wait_port_free(SERIAL_PORT)
    wait_port_free(MON_PORT)
    ensure_disks()

    verdict = True

    # ---- off 态（repo 根 conf；真机等价物）----
    log_off = run_case("off", CONF_ROOT, os.path.join(ATTIC, "varix-gate-off.iso"), timeout)
    must = ["nvme: write probes SKIPPED (storage_selftest off - real-disk protection)",
            "nvme: init ok"]
    forbid = ["nvme: loopback", "fs23-disk", "milestone: M2"]
    print("\n=== off 态（默认，真机等价）===")
    for m in must:
        ok = grep_log(log_off, m) > 0
        verdict &= ok
        print("  [MUST ] %-72s %s" % (m, "HIT" if ok else "MISS"))
    for m in forbid:
        ok = grep_log(log_off, m) == 0
        verdict &= ok
        print("  [FORBID] %-69s %s" % (m, "CLEAN" if ok else "FOUND!!"))

    # ---- on 态（QEMU selftest conf）----
    # 断言子串 = 各探针真实日志前缀（09:5x 串口实测）：
    #   kvsrv→"kvsrv:"、vfsguard→"vfs-probe: audit-full"、kvault→"vault: PROBE PASS"。
    # kvsrv 的 session 行为学细节（脏测试盘上 set counter 可能 WARN 退出）
    # 属 qemu-shell-walkthrough 验收口径，不属本门禁口径。
    log_on = run_case("on", CONF_SELFTEST, os.path.join(ATTIC, "varix-gate-on.iso"), timeout)
    must_on = [
        "nvme: storage_selftest=1",
        "nvme: loopback x1000 passed=true",
        "fs23-disk verify verdict=ok",
        "milestone: M2",
        "kvsrv:",
        "vfs-probe: audit-full",
        "vault: PROBE PASS",
        "SHELL: desktop-ready",
    ]
    print("\n=== on 态（storage_selftest=1，QEMU 刮擦盘）===")
    for m in must_on:
        ok = grep_log(log_on, m) > 0
        verdict &= ok
        print("  [MUST ] %-72s %s" % (m, "HIT" if ok else "MISS"))

    # on 态绝不允许 SKIPPED（开关必须真的打开探针）
    ok = grep_log(log_on, "write probes SKIPPED") == 0
    verdict &= ok
    print("  [FORBID] %-69s %s" % ("write probes SKIPPED", "CLEAN" if ok else "FOUND!!"))

    print("\n" + "=" * 64)
    print("STORAGE-GATE VERDICT:", "PASS" if verdict else "FAIL")
    print("=" * 64)
    return 0 if verdict else 1


if __name__ == "__main__":
    sys.exit(main())
