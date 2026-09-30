"""任务21 里程碑整合演示驱动（双会话断电恢复）。

整链叙事：内核可运行用户态程序读写真盘——
  会话1（空盘）：M2 journal format+封条3条 → M3 exFAT 读 /apps.json →
        M4 快照区写 marker → M1 ring3 用户程序（task15 lifecycle）→ halting
        → 本脚本 kill QEMU = 模拟断电（NVMe 盘面跨进程持久）。
  会话2（同盘面重启）：M2 journal 恢复 verdict=ok → M3 exFAT 读 →
        M4 snapshot-persisted ok（会话1 marker 仍在盘面）→ M1 ring3 再跑
        → MILESTONE DEMO PASS。

用法：python _attic/milestone-demo.py
前置：cargo kbuild + python scripts/make-iso-qemu.py（varix-qemu.iso 含 milestone 探针）。
"""

import os
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ATTIC = os.path.join(ROOT, "_attic")
ISO = os.path.join(ROOT, "varix-qemu.iso")
INIT_IMG = os.path.join(ATTIC, "ms21-init.img")
SHARED_IMG = os.path.join(ATTIC, "ms21-shared.img")
SERIAL1 = os.path.join(ATTIC, "ms21-session1.log")
SERIAL2 = os.path.join(ATTIC, "ms21-session2.log")
MON_PORT = 14449

INIT_BLOCKS = 1_048_576  # 512 MiB / 512B

# 会话1：走 fresh 路径（封条 → ready-for-powercut），ring3 用户程序跑完。
S1_PRESENT = [
    "milestone: M2 journal fresh",
    "milestone: M2 journal sealed entries=3",
    "milestone: ready-for-powercut",
    "milestone: M3 exfat-read ok /apps.json",
    "milestone: M4 snapshot written",
    "ring3: task15 lifecycle complete",
]
# 会话2：走恢复路径（journal 恢复 + 快照持久），ring3 再跑。
S2_PRESENT = [
    "milestone: M2 journal open entries=3 torn=0",
    "milestone: journal-recovered verdict=ok",
    "milestone: M3 exfat-read ok /apps.json",
    "milestone: snapshot-persisted ok",
    "ring3: task15 lifecycle complete",
]
S1_ABSENT = ["journal-recovered", "snapshot-persisted ok"]  # 会话1 不得出现的恢复标志
TIMEOUT_S = 300


def read_log(path):
    try:
        with open(path, "r", errors="replace") as f:
            return f.read()
    except OSError:
        return ""


def milestone_lines(text):
    return [ln for ln in text.splitlines() if "milestone:" in ln]


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
            "-device", "nvme,drive=nv1,serial=MS21INIT",
            "-drive", "file=%s,if=none,id=nv2,format=raw" % SHARED_IMG,
            "-device", "nvme,drive=nv2,serial=MS21SHRD",
            "-monitor", "tcp:127.0.0.1:%d,server,nowait" % MON_PORT,
        ],
        cwd=str(ROOT),
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def power_cut(proc):
    """kill -9 等价：整机断电，盘面（镜像文件）跨进程持久。"""
    proc.kill()
    proc.wait()
    time.sleep(1)


def run_session(tag, serial_log, present, absent, timeout_s):
    """跑一个 QEMU 会话：等全部 present 标志 + 无 absent 标志 + 无里程碑级失败。"""
    proc = launch_qemu(serial_log)
    print("[demo] %s launched pid=%d; waiting for %d markers ..." % (tag, proc.pid, len(present)), flush=True)
    missing = list(present)
    deadline = time.time() + timeout_s
    while time.time() < deadline and missing:
        text = read_log(serial_log)
        missing = [m for m in present if m not in text]
        for ln in milestone_lines(text):
            bad = ("failed" in ln or "tainted" in ln or "NOT rejected" in ln)
            if bad:
                power_cut(proc)
                print("[demo] %s FAIL — milestone failure line:" % tag, flush=True)
                print("       " + ln, flush=True)
                return None
        if any(m in text for m in absent):
            power_cut(proc)
            print("[demo] %s FAIL — session-1 log contains recovery marker:" % tag, flush=True)
            return None
        time.sleep(2)
    power_cut(proc)
    if missing:
        print("[demo] %s TIMEOUT — missing markers:" % tag, flush=True)
        for m in missing:
            print("       - " + m, flush=True)
        print("[demo] --- milestone lines seen ---", flush=True)
        for ln in milestone_lines(read_log(serial_log)):
            print("       " + ln, flush=True)
        return None
    print("[demo] %s all markers present; power-cut OK" % tag, flush=True)
    return read_log(serial_log)


def main():
    # 0) 全新盘面：init 盘空 512MiB + shared 卷重造（无历史 marker/日志）。
    with open(INIT_IMG, "wb") as f:
        f.truncate(INIT_BLOCKS * 512)
    r = subprocess.run(
        [sys.executable, os.path.join(ATTIC, "mkexfat.py"), SHARED_IMG],
        capture_output=True, text=True,
    )
    if r.returncode != 0:
        print("[demo] mkexfat failed:\n" + r.stdout + r.stderr)
        return 1
    print("[demo] fresh disks ready (init 512MiB + shared exFAT)", flush=True)

    # 1) 会话1：空盘封条 → 用户程序 → 断电。
    t1 = run_session("session-1", SERIAL1, S1_PRESENT, S1_ABSENT, TIMEOUT_S)
    if t1 is None:
        return 1
    # 2) 会话2：同盘面重启 → 恢复 → PASS。
    t2 = run_session("session-2", SERIAL2, S2_PRESENT, [], TIMEOUT_S)
    if t2 is None:
        return 1

    print("[demo] ===== MILESTONE DEMO PASS =====", flush=True)
    print("[demo] session-1 milestone lines:", flush=True)
    for ln in milestone_lines(t1):
        print("       " + ln, flush=True)
    print("[demo] session-2 milestone lines:", flush=True)
    for ln in milestone_lines(t2):
        print("       " + ln, flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
