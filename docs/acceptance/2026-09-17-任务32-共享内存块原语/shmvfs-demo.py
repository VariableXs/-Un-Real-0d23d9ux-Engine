"""任务30/32 实机驱动（双会话断电恢复）。

会话1（全新盘面）：shm-probe 授权矩阵全绿 + vfs 裁决矩阵全绿 +
        审计账本 format→6 条 → `vfs-probe: audit-session=1 entries=6 verdict=ok`
        → ring3 → 脚本 kill QEMU = 断电。
会话2（同盘面）：审计账本重开重放 → 前会话 6 条零丢失逐条核对 →
        `vfs-probe: audit-recovered verdict=ok recovered=6` + `audit-session=2 entries=12`。
shm/decisions 探针每会话幂等复跑（verdict=true/ok 双证）。
"""

import os
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ATTIC = os.path.join(ROOT, "_attic")
ISO = os.path.join(ROOT, "varix-qemu.iso")
INIT_IMG = os.path.join(ATTIC, "s3032-init.img")
SHARED_IMG = os.path.join(ATTIC, "shared-exfat.img")
SERIAL1 = os.path.join(ATTIC, "shmvfs-session1.log")
SERIAL2 = os.path.join(ATTIC, "shmvfs-session2.log")
MON_PORT = 14455

INIT_BLOCKS = 1_048_576  # 512 MiB / 512B

S1_PRESENT = [
    "shm-probe: verdict=true",
    "vfs-probe: decisions",
    "vfs-probe: audit-session=1 entries=6 verdict=ok",
    "ring3: task15 lifecycle complete",
]
S2_PRESENT = [
    "vfs-probe: audit-recovered verdict=ok recovered=6",
    "vfs-probe: audit-session=2 entries=12 verdict=ok",
    "shm-probe: verdict=true",
    "ring3: task15 lifecycle complete",
]
S1_ABSENT = ["audit-recovered"]
TIMEOUT_S = 300


def read_log(path):
    try:
        with open(path, "r", errors="replace") as f:
            return f.read()
    except OSError:
        return ""


def launch_qemu(serial_log):
    if os.path.exists(serial_log):
        os.remove(serial_log)
    return subprocess.Popen(
        [
            "qemu-system-x86_64",
            "-cdrom", ISO,
            "-serial", "file:" + serial_log,
            "-no-reboot", "-no-shutdown",
            "-m", "512M", "-M", "q35", "-display", "none",
            "-boot", "order=d",
            "-drive", "file=%s,if=none,id=nv1,format=raw" % INIT_IMG,
            "-device", "nvme,drive=nv1,serial=S3032INIT",
            "-drive", "file=%s,if=none,id=nv2,format=raw" % SHARED_IMG,
            "-device", "nvme,drive=nv2,serial=S3032SHRD",
            "-monitor", "tcp:127.0.0.1:%d,server,nowait" % MON_PORT,
        ],
        cwd=str(ROOT),
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def power_cut(proc):
    proc.kill()
    proc.wait()
    time.sleep(1)


def run_session(tag, serial_log, present, absent, timeout_s):
    proc = launch_qemu(serial_log)
    print("[demo] %s launched pid=%d; waiting for %d markers ..." % (tag, proc.pid, len(present)), flush=True)
    missing = list(present)
    deadline = time.time() + timeout_s
    text = ""
    while time.time() < deadline and missing:
        text = read_log(serial_log)
        missing = [m for m in present if m not in text]
        for ln in text.splitlines():
            bad = (
                ("shm-probe:" in ln and ("FAIL" in ln or "false" in ln))
                or ("vfs-probe:" in ln and "FAIL" in ln)
                or ("kvsrv:" in ln and "failed" in ln)
                or "panicked at" in ln
                or "PANIC" in ln
            )
            if bad:
                power_cut(proc)
                print("[demo] %s FAIL — failure line:" % tag, flush=True)
                print("       " + ln, flush=True)
                return None
        if any(m in text for m in absent):
            power_cut(proc)
            print("[demo] %s FAIL — session-1 log contains session-2 marker:" % tag, flush=True)
            return None
        time.sleep(2)
    power_cut(proc)
    if missing:
        print("[demo] %s TIMEOUT — missing markers:" % tag, flush=True)
        for m in missing:
            print("       - " + m, flush=True)
        print("[demo] --- probe lines seen ---", flush=True)
        for ln in text.splitlines():
            if "shm-probe" in ln or "vfs-probe" in ln:
                print("       " + ln, flush=True)
        return None
    print("[demo] %s all markers present; power-cut OK" % tag, flush=True)
    return read_log(serial_log)


def probe_lines(text):
    return [ln for ln in text.splitlines() if ("shm-probe" in ln or "vfs-probe" in ln)]


def main():
    # 全新盘面：init 盘空 512MiB + shared 卷重造（milestone 共享探针保持绿）。
    with open(INIT_IMG, "wb") as f:
        f.truncate(INIT_BLOCKS * 512)
    r = subprocess.run(
        [sys.executable, os.path.join(ATTIC, "mkexfat.py"), SHARED_IMG],
        capture_output=True, text=True,
    )
    if r.returncode != 0:
        print("[demo] mkexfat failed:\n" + r.stdout + r.stderr)
        return 1
    print("[demo] fresh disks ready", flush=True)

    t1 = run_session("session-1", SERIAL1, S1_PRESENT, S1_ABSENT, TIMEOUT_S)
    if t1 is None:
        return 1
    t2 = run_session("session-2", SERIAL2, S2_PRESENT, [], TIMEOUT_S)
    if t2 is None:
        return 1

    print("[demo] ===== SHM+VFS POWERCUT DEMO PASS =====", flush=True)
    print("[demo] session-1 probe lines:", flush=True)
    for ln in probe_lines(t1):
        print("       " + ln, flush=True)
    print("[demo] session-2 probe lines:", flush=True)
    for ln in probe_lines(t2):
        print("       " + ln, flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
