#!/usr/bin/env python3
"""任务10（AI-B）· 实机 U 盘强拔演练（物理拔出）——拔盘轮 + 恢复轮。

与 QEMU 先行演练（HMP drive_del）的差异验证点：物理拔出让宿主句柄失效，
guest 收到的是真实控制器/IO 错误路径（非 QEMU 模拟的介质即刻消失）。

流程：
1. QEMU 直通 U 盘（\\.\E: 卷设备只读，零写入保护数据）
2. 等串口 `nvme: init ok`（内核已识别 U 盘）
3. 轮询 E: 存在性——用户任意时刻拔出即被精确感知（PULL 窗口 300s）
4. 拔盘后观察 40s，HMP info block 存证
5. 判定：PANIC/fatal/#DF/triple fault/rebooting 全零

用法：python _attic/usb-unplug-drill.py
证据：_attic/usb63-drill-serial.log + _attic/usb63-drill-hmp.txt
"""
import os
import socket
import subprocess
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ATTIC = os.path.join(ROOT, "_attic")
ISO = os.path.join(ROOT, "varix-qemu.iso")
SERIAL = os.path.join(ATTIC, "usb63-drill-serial.log")
HMP_OUT = os.path.join(ATTIC, "usb63-drill-hmp.txt")
MON_PORT = 14674
PULL_TIMEOUT_S = 300   # 等待用户拔盘窗口
OBSERVE_AFTER_S = 40   # 拔盘后观察窗口
BAD_WORDS = ("PANIC", "panic", "fatal", "#DF", "triple fault", "rebooting")


def read_log():
    try:
        with open(SERIAL, "r", errors="replace") as f:
            return f.read()
    except OSError:
        return ""


def usb_present():
    return os.path.exists("E:\\")


def hmp_info_block():
    try:
        c = socket.create_connection(("127.0.0.1", MON_PORT), timeout=5)
        c.settimeout(5)
        buf = b""
        while b"(qemu)" not in buf:
            buf += c.recv(4096)
        c.sendall(b"info block\n")
        out = b""
        while b"(qemu)" not in out:
            out += c.recv(65536)
        c.close()
        return out.decode(errors="replace")
    except Exception as e:
        return "HMP capture failed: %s" % e


def main():
    if not os.path.exists(ISO):
        print("missing ISO:", ISO)
        return 2
    if not usb_present():
        print("E: not present - U 盘未插入")
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
            "-drive", r"file=\\.\E:,if=none,id=nv1,format=raw,readonly=on",
            "-device", "nvme,drive=nv1,serial=USBTU200",
            "-monitor", "tcp:127.0.0.1:%d,server,nowait" % MON_PORT,
        ],
        cwd=ROOT,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    print("qemu pid:", proc.pid)
    # ① 等 NVMe 枚举完成（内核已识别 U 盘）。
    t0 = time.time()
    while time.time() - t0 < 120:
        if "nvme: init ok" in read_log():
            print("[T+%ds] nvme init ok - 内核已识别 U 盘" % int(time.time() - t0))
            break
        time.sleep(1)
    else:
        print("120s 内未见 nvme: init ok（继续进入等待拔盘阶段）")
    # ② 等待物理拔盘（轮询 E: 消失）。
    print(">>> 等待拔盘：请任意时刻拔出 U 盘（窗口 %ds） <<<" % PULL_TIMEOUT_S)
    t1 = time.time()
    pulled_at = None
    while time.time() - t1 < PULL_TIMEOUT_S:
        if not usb_present():
            pulled_at = time.time()
            print("[T+%ds] *** U 盘已拔出（物理移除感知）***" % int(pulled_at - t0))
            break
        time.sleep(0.5)
    if pulled_at is None:
        print("窗口期内未检测到拔盘——如实报告，演练未完成拔盘动作")
    # ③ 拔盘后观察窗口。
    print("拔盘后观察 %ds ..." % OBSERVE_AFTER_S)
    time.sleep(OBSERVE_AFTER_S)
    # ④ HMP 设备状态存证。
    info = hmp_info_block()
    with open(HMP_OUT, "w", encoding="utf-8") as f:
        f.write(info)
    print("=== HMP info block（节选）===")
    for line in info.splitlines()[:12]:
        print(line)
    # ⑤ 收尾判定。
    time.sleep(2)
    log = read_log()
    proc.terminate()
    try:
        proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        proc.kill()
    tail_start = max(0, len(log) - 6000) if pulled_at else 0
    tail = log[tail_start:]
    bad = [w for w in BAD_WORDS if w in tail]
    print("=== 判定 ===")
    print("拔盘动作:", "已执行" if pulled_at else "未执行")
    print("异常词扫描（拔盘后串口尾段）:", bad if bad else "全零（零 panic/零 fatal/零 #DF）")
    print("串口总长:", len(log), "chars；证据:", SERIAL, "+", HMP_OUT)
    return 0 if (pulled_at and not bad) else 1


if __name__ == "__main__":
    import sys
    sys.exit(main())
