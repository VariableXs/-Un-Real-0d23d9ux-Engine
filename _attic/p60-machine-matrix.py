#!/usr/bin/env python3
"""任务60（AI-P）· 多机型测试矩阵——QEMU 代理矩阵首轮（实机=待协作项）。

机型代理三配置（与总案「QEMU 可完成项先行、实机集中执行」解卡规则一致）：
  A) q35 · 单核 · 1GiB —— 全功能基线（任务 27/28 走查同款）
  B) q35 · 双核 · 2GiB —— 多核/大内存调度代理（-smp 2）
  C) pc(i440FX) · 单核 · 1GiB —— 无 ECAM/MCFG 诚实降级路径
     （块栈如实跳过、内核继续引导，验证「降级不致瘫」）

每配置断言（各自口径）：
  A/B：boot completed + desktop-ready + SHARED 挂载 + 零 panic。
  C  ：boot completed + 「no MCFG table - block stack skipped」如实打点
       + 零 panic（块栈降级但内核存活）。

证据：_attic/p60-matrix/matrix-A/B/C-serial.log + p60-matrix-report.json
实机 ≥3 台不同厂商：登记为待用户协作项（任务 10/63 已有实机协作先例）。
"""
import json
import os
import socket
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ATTIC = os.path.join(ROOT, "_attic")
ISO = os.path.join(ROOT, "varix-qemu.iso")
TEST_IMG = os.path.join(ATTIC, "p27-testdisk.img")
SHARED_IMG = os.path.join(ATTIC, "p27-shared-exfat.img")
OUTDIR = os.path.join(ATTIC, "p60-matrix")
REPORT = os.path.join(OUTDIR, "p60-matrix-report.json")
MON_PORT = 14741

BOOT_TIMEOUT = 600
STEP_TIMEOUT = 90
BAD = ("kernel panic", "triple fault", "#DF")


def read_log(path):
    try:
        with open(path, "r", errors="replace") as f:
            return f.read()
    except OSError:
        return ""


def wait_for(path, marker, timeout=BOOT_TIMEOUT):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if marker in read_log(path):
            return True
        time.sleep(0.1)
    return False


class Mon:
    """HMP 客户端（原始 socket 收发，与 p58 演练同款）。"""

    def __init__(self, port):
        for _ in range(50):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=3)
                self.s.settimeout(0.4)
                break
            except OSError:
                time.sleep(0.2)
        else:
            raise RuntimeError("monitor connect failed")
        self._drain(1.0)

    def _drain(self, budget_s):
        out = []
        deadline = time.time() + budget_s
        while time.time() < deadline:
            try:
                chunk = self.s.recv(65536)
                if not chunk:
                    break
                out.append(chunk.decode("utf-8", "replace"))
            except socket.timeout:
                continue
            except OSError:
                break
        return "".join(out)

    def cmd(self, line, settle=0.4):
        self.s.sendall((line + "\n").encode())
        return self._drain(settle + 0.3)

    def key(self, k):
        self.cmd("sendkey " + k, settle=0.2)


CONFIGS = [
    ("A-q35-baseline", ["-machine", "q35"], True),
    ("B-q35-smp2", ["-machine", "q35", "-smp", "2", "-m", "2048"], True),
    ("C-i440fx-degraded", ["-machine", "pc"], False),
]


def run_one(name, extra, full):
    serial = os.path.join(OUTDIR, f"matrix-{name}-serial.log")
    if os.path.exists(serial):
        os.remove(serial)
    args = ["qemu-system-x86_64", *extra,
            "-cdrom", ISO,
            "-drive", f"file={TEST_IMG},if=none,id=nv1,format=raw",
            "-device", "nvme,drive=nv1,id=nv1dev,serial=P60A",
            "-drive", f"file={SHARED_IMG},if=none,id=nv2,format=raw",
            "-device", "nvme,drive=nv2,id=nv2dev,serial=P60B",
            "-serial", "file:" + serial,
            "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait",
            "-m", "1024", "-no-reboot", "-no-shutdown"]
    proc = subprocess.Popen(args, cwd=ROOT)
    row = {"name": name, "boot": False, "checks": []}
    try:
        boot = wait_for(serial, "boot completed")
        row["boot"] = boot
        log = read_log(serial)
        if full:
            # ushell 在 boot-replay done 后必须先收任意键才进桌面
            # （与 p58 演练 boot_to_desktop 同一约定，否则双向死等）。
            replay = wait_for(serial, "SHELL: boot-replay done", STEP_TIMEOUT)
            if replay:
                time.sleep(1.0)
                mon = Mon(MON_PORT)
                mon.key("spc")
            ok_desktop = wait_for(serial, "SHELL: desktop-ready", STEP_TIMEOUT)
            ok_shared = "source=shared-exfat" in read_log(serial)
            log = read_log(serial)  # 全链走完后再取异常词快照
            row["checks"] = [
                ("boot completed", boot),
                ("desktop-ready", ok_desktop),
                ("SHARED mounted", ok_shared),
                ("no panic", not any(b in log for b in BAD)),
            ]
        else:
            skipped = "no MCFG table - block stack skipped" in log
            row["checks"] = [
                ("boot completed", boot),
                ("block stack honestly skipped", skipped),
                ("no panic", not any(b in log for b in BAD)),
            ]
        row["verdict"] = "PASS" if all(ok for _, ok in row["checks"]) else "FAIL"
    finally:
        try:
            proc.kill()
        except OSError:
            pass
    print(f"  [{name}] {row['verdict']}  " +
          " ".join(f"{n}={'PASS' if ok else 'FAIL'}" for n, ok in row["checks"]))
    return row


def main():
    os.makedirs(OUTDIR, exist_ok=True)
    if not os.path.exists(TEST_IMG):
        with open(TEST_IMG, "wb") as f:
            f.truncate(1_048_576 * 512)
            f.seek(60_000 * 512)
            f.write(b"P2728-PROBE".ljust(512, b"0"))
    print("=== 任务60 · 机型代理矩阵首轮 ===")
    rows = [run_one(name, extra, full) for name, extra, full in CONFIGS]
    all_ok = all(r["verdict"] == "PASS" for r in rows)
    report = {
        "round": "QEMU 代理矩阵首轮（2026-09-18）",
        "rows": rows,
        "physical_machines": "待用户协作项：≥3 台不同厂商实机（任务10/63 同批协作通道）",
        "verdict": "PASS" if all_ok else "FAIL",
    }
    with open(REPORT, "w", encoding="utf-8") as f:
        json.dump(report, f, ensure_ascii=False, indent=1)
    print("MATRIX ROUND-1:", report["verdict"])
    return 0 if all_ok else 1


if __name__ == "__main__":
    sys.exit(main())
