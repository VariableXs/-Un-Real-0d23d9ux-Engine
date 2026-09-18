#!/usr/bin/env python3
"""任务65（AI-B）· 保险箱内核侧实机演示驱动。

单会话全链：NVMe 探针链走到 vault_probe（LBA 90000 区）——
init(PBKDF2 100k) → 锁定断言 → 错口令拒 → 解锁 → put/get 往返 →
盘面密文无明文 → 焚毁三步 → 恢复尝试失败 → lock 零化 → PROBE PASS。

用法：python _attic/vault-demo.py
证据：_attic/vault65-serial.log + docs/acceptance/2026-09-17-任务65-保险箱内核侧/
"""
import os
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ATTIC = os.path.join(ROOT, "_attic")
ISO = os.path.join(ROOT, "varix-qemu.iso")
INIT_IMG = os.path.join(ATTIC, "p65-testdisk.img")
SERIAL = os.path.join(ATTIC, "vault65-serial.log")
MON_PORT = 14661

# 探针链在 vault 之前还有 NVMe/input/display/quota/shm/vfs 一整串，
# PBKDF2 100k×2 在 TCG 下约 1-3 分钟，总预算给 12 分钟。
TIMEOUT_S = 720
MARKERS = [
    "vault: PROBE PASS",
    "vault: PROBE FAIL",
]


def read_log():
    try:
        with open(SERIAL, "r", errors="replace") as f:
            return f.read()
    except OSError:
        return ""


def make_disk():
    """自建 512MiB 探针盘（LBA 90000+ 可达；仓库根 nvme0.img 仅 16MiB 不够）。"""
    with open(INIT_IMG, "wb") as f:
        f.truncate(1_048_576 * 512)
        f.seek(60_000 * 512)
        f.write(b"P65-VAULT-PROBE".ljust(512, b"0"))


def main():
    if not os.path.exists(ISO):
        print("missing ISO:", ISO)
        return 2
    make_disk()
    if os.path.exists(SERIAL):
        os.remove(SERIAL)
    proc = subprocess.Popen(
        [
            "qemu-system-x86_64",
            "-cdrom", ISO,
            "-serial", "file:" + SERIAL,
            "-no-reboot", "-no-shutdown",
            "-m", "512M", "-M", "q35", "-display", "none",
            "-boot", "order=d",
            "-drive", "file=%s,if=none,id=nv1,format=raw" % INIT_IMG,
            "-device", "nvme,drive=nv1,serial=KV24INIT",
            "-monitor", "tcp:127.0.0.1:%d,server,nowait" % MON_PORT,
        ],
        cwd=ROOT,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    print("qemu pid:", proc.pid, "waiting for vault probe (timeout %ds)" % TIMEOUT_S)
    t0 = time.time()
    hit = None
    while time.time() - t0 < TIMEOUT_S:
        log = read_log()
        for m in MARKERS:
            if m in log:
                hit = m
                break
        if hit:
            break
        time.sleep(5)
    # 未命中标记：抓寄存器现场（卡点 rip → 符号表对照）再收尾。
    if not hit:
        try:
            import socket as _s
            c = _s.create_connection(("127.0.0.1", MON_PORT), timeout=5)
            c.settimeout(5)
            buf = b""
            while b"(qemu)" not in buf:
                buf += c.recv(4096)
            c.sendall(b"info registers" + b"\n")
            regs = b""
            while b"(qemu)" not in regs:
                regs += c.recv(65536)
            open(os.path.join(ATTIC, "vault65-hang-regs.txt"), "wb").write(buf + regs)
            c.close()
            print("--- hang registers saved: _attic/vault65-hang-regs.txt ---")
            print(regs.decode(errors="replace")[:1200])
        except Exception as e:
            print("HMP capture failed:", e)
    # 再给 2 秒串口缓冲。
    time.sleep(2)
    log = read_log()
    proc.terminate()
    try:
        proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        proc.kill()

    # 摘取 vault 段落。
    lines = [l for l in log.splitlines() if "vault" in l.lower() or "nvme: loopback" in l]
    for l in lines[-40:]:
        print(l)
    if hit == "vault: PROBE PASS":
        print("\nRESULT: PASS")
        return 0
    if hit == "vault: PROBE FAIL":
        print("\nRESULT: FAIL")
        return 1
    print("\nRESULT: TIMEOUT (no marker in %ds)" % TIMEOUT_S)
    return 3


if __name__ == "__main__":
    sys.exit(main())
