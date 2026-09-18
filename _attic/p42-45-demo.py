#!/usr/bin/env python3
"""任务42-45（AI-B）· 实机联合探针驱动。

Job 限额（mem-limit/零泄漏/cpu-rate 窗口滚动）+ prefix 每进程隔离
（create/resolve/escape-refused/destroy）+ compatdb 自动裁决
（notepad=Partial / chat-legacy=Refused）+ KILL_ON_JOB_CLOSE 边界击杀。

用法：python _attic/p42-45-demo.py
证据：_attic/p42-45-serial.log
"""
import os
import subprocess
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ATTIC = os.path.join(ROOT, "_attic")
ISO = os.path.join(ROOT, "varix-qemu.iso")
INIT_IMG = os.path.join(ATTIC, "p65-testdisk.img")
SERIAL = os.path.join(ATTIC, "p42-45-serial.log")
MON_PORT = 14671

TIMEOUT_S = 900
MARKERS_OK = "ring3: task15 lifecycle complete"
MARKERS_BAD = ("VERDICT=FAIL", "fatal", "kernel panic")


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
        f.write(b"P4245-PROBE".ljust(512, b"0"))


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
            "-drive", f"file={INIT_IMG},if=none,id=nv1,format=raw",
            "-device", "nvme,drive=nv1,serial=P4245",
            "-serial", "file:" + SERIAL,
            "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait",
            "-m", "1024",
            "-no-reboot", "-no-shutdown",
        ],
        cwd=ROOT,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    print("qemu pid", proc.pid, "- waiting for markers...")
    hit = False
    start = time.time()
    try:
        while time.time() - start < TIMEOUT_S:
            log = read_log()
            if MARKERS_OK in log or any(b in log for b in MARKERS_BAD):
                hit = True
                break
            time.sleep(2)
    finally:
        try:
            proc.kill()
        except OSError:
            pass
    log = read_log()
    checks = [
        "notepad: open\u2192edit\u2192save closed loop VERDICT=PASS" in log,
        "job-probe: mem-limit" in log and "verdict=ok" in log.split("job-probe: mem-limit")[1][:120],
        "job-probe: table slots_used=0" in log and "verdict=ok" in log.split("job-probe: table slots_used=0")[1][:80],
        "job-probe: cpu-rate" in log and "verdict=ok" in log.split("job-probe: cpu-rate")[1][:140],
        "prefix-probe:" in log and "verdict=ok" in log.split("prefix-probe:")[1][:120],
        "compatdb-probe:" in log and "verdict=ok" in log.split("compatdb-probe:")[1][:160],
        "jobkill" in log and "verdict=ok" in log.split("jobkill")[1][:400] if "jobkill" in log else False,
        MARKERS_OK in log,
        not any(b in log for b in MARKERS_BAD),
    ]
    print("checks:", [i + 1 for i, c in enumerate(checks) if c], "of", len(checks))
    for line in log.splitlines():
        if any(k in line for k in ("job-probe", "prefix-probe", "compatdb-probe", "jobkill",
                                   "VERDICT", "lifecycle complete", "verdict")):
            print(line.strip())
    return 0 if all(checks) else 1


if __name__ == "__main__":
    raise SystemExit(main())
