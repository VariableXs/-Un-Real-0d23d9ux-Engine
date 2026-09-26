"""任务56 配额服务实机驱动（单会话纯计算探针）。

探针四步：①矩阵饱和份额+保底 ②单方独占压测 ③水位三档演练+归还
④GPU 通道预留/记账。全部标志就位 → QUOTA PROBE PASS。
"""

import os
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ATTIC = os.path.join(ROOT, "_attic")
ISO = os.path.join(ROOT, "varix-qemu.iso")
SERIAL = os.path.join(ATTIC, "quota56-session1.log")
MON_PORT = 14453

PRESENT = [
    "quota: matrix shares",
    "quota: monopoly",
    "quota: watermark drill",
    "quota: hysteresis",
    "quota: gpu",
    "QUOTA PROBE PASS",
    "ring3: task15 lifecycle complete",
]
ABSENT = ["verdict=FAIL", "QUOTA PROBE FAIL"]
TIMEOUT_S = 240


def read_log(path):
    try:
        with open(path, "r", errors="replace") as f:
            return f.read()
    except OSError:
        return ""


def main():
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
            "-monitor", "tcp:127.0.0.1:%d,server,nowait" % MON_PORT,
        ],
        cwd=str(ROOT),
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    print("[demo] launched pid=%d; waiting for %d markers ..." % (proc.pid, len(PRESENT)), flush=True)
    text = ""
    missing = list(PRESENT)
    deadline = time.time() + TIMEOUT_S
    fail_line = None
    while time.time() < deadline and missing:
        text = read_log(SERIAL)
        missing = [m for m in PRESENT if m not in text]
        for ln in text.splitlines():
            if ("quota:" in ln and "FAIL" in ln) or "panicked at" in ln or "PANIC" in ln:
                fail_line = ln
                break
        if fail_line:
            break
        time.sleep(2)
    proc.kill()
    proc.wait()
    time.sleep(1)
    text = read_log(SERIAL)

    if fail_line:
        print("[demo] FAIL — failure line: " + fail_line)
        return 1
    if missing:
        print("[demo] TIMEOUT — missing markers:")
        for m in missing:
            print("       - " + m)
        print("[demo] --- quota lines seen ---")
        for ln in text.splitlines():
            if "quota" in ln.lower():
                print("       " + ln)
        return 1
    print("[demo] all markers present")
    print("[demo] --- quota probe lines (serial) ---")
    for ln in text.splitlines():
        if "quota" in ln.lower():
            print("       " + ln)
    panic = [ln for ln in text.splitlines() if "PANIC" in ln or "panicked at" in ln]
    print("[demo] PANIC count = %d" % len(panic))
    print("[demo] ===== QUOTA PROBE DEMO PASS =====")
    return 0


if __name__ == "__main__":
    sys.exit(main())
