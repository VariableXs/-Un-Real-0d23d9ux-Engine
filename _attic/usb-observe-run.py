#!/usr/bin/env python3
"""任务10/63（AI-B）· 实机 U 盘强拔演练（物理拔出）——观察跑。

QEMU 直通物理 U 盘（\\.\PhysicalDrive1, 只读+locking=off，零写入保护 U 盘），
观察内核对 U 盘的真实反应（NVMe 枚举/挂载/读 IO），为物理拔盘演练定标记。

用法：python _attic/usb-observe-run.py
证据：_attic/usb63-observe-serial.log
"""
import os
import subprocess
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ATTIC = os.path.join(ROOT, "_attic")
ISO = os.path.join(ROOT, "varix-qemu.iso")
SERIAL = os.path.join(ATTIC, "usb63-observe-serial.log")
MON_PORT = 14673
OBSERVE_S = 90  # 观察窗口：引导 + 内核对 U 盘的完整反应


def read_log():
    try:
        with open(SERIAL, "r", errors="replace") as f:
            return f.read()
    except OSError:
        return ""


def main():
    if not os.path.exists(ISO):
        print("missing ISO:", ISO)
        return 2
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
            # 实机 U 盘直通：卷设备路径（非提权可读打开，probe-physdrive.py 已验证）。
            # readonly=on 零写入；locking=off 绕过 Windows 卷挂载锁。
            # 备选（需管理员）：\\.\PhysicalDrive1 整盘路径。
            "-drive", r"file=\\.\E:,if=none,id=nv1,format=raw,readonly=on",
            "-device", "nvme,drive=nv1,serial=USBTU200",
            "-monitor", "tcp:127.0.0.1:%d,server,nowait" % MON_PORT,
        ],
        cwd=ROOT,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
    )
    print("qemu pid:", proc.pid, "observing %ds" % OBSERVE_S)
    time.sleep(OBSERVE_S)
    if proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()
    err = b""
    try:
        err = proc.stderr.read() or b""
    except Exception:
        pass
    log = read_log()
    print("=== qemu stderr (前 800B) ===")
    print(err.decode(errors="replace")[:800])
    print("=== serial %d chars，关键行 ===" % len(log))
    keys = ("nvme", "NVMe", "nv1", "USBTU200", "exfat", "exFAT", "mount", "probe",
            "PANIC", "panic", "fatal", "boot", "lifecycle", "kv", "fs23")
    shown = 0
    for line in log.splitlines():
        if any(k in line for k in keys) and shown < 60:
            print(line)
            shown += 1
    return 0


if __name__ == "__main__":
    sys_exit = main()
    import sys
    sys.exit(sys_exit)
