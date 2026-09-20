#!/usr/bin/env python3
"""键盘失联排障 + RESTART 复位冒烟（2026-09-20）——BootScreen 可退出、
键盘诊断上屏、开始菜单 Restart 一键复位回 Windows 引导序。

背景：真机（Y7000）BootScreen「press any key」全无响应、卡死。改动链：
  ① kernel ps2.controller_init：启动期 i8042 标准初始化（自检/接口测试/
     配置回写/开端口），全部限次自旋不挂死；
  ② input_probe：真机（无 hypervisor 位）跳过 90s HMP 等待窗口；
  ③ boot://event 第 15 条记录（idx=14）：活时钟 ms + kbd 诊断字
     （控制器探针 + 已收原始字节计数 + 最后原始字节）；
  ④ ushell BootScreen：诊断行 + 倒计时行实时刷新，10s 无键自动进桌面；
  ⑤ ushell 开始菜单第四项 Restart → SYS_REBOOT(19)：UEFI ResetSystem
     （RS 恒等映射后）+ 8042 脉冲兜底，复位回固件默认引导序（Windows）。

本脚本验证链（QEMU）：
  A. kbd-init probe 行出现（controller_init 已跑）；
  B. input-probe verdict=true（控制器初始化后 QEMU 键鼠注入路径零回归，
     提前退出 90s 窗口）；
  C. BootScreen 全程**不按任何键** → 10s 超时自动 desktop-ready（兜底路径）；
  D. 桌面再按键 → startmenu opened（键路径活着）；
  E. BootScreen 期间两张 screendump：诊断行 + 倒计时数字在走；
  F. 开始菜单下移三次选中 Restart → Enter → 串口 reboot 阶梯行出现，
     随后**同一串口日志出现第二次 kbd-init boot 链**=整机真重启送达。

用法：python _attic/qemu-kbd-smoke.py
证据：_attic/qemu-kbd-serial.log + _attic/acceptance-kbd/*.png
"""
import os
import socket
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ATTIC = os.path.join(ROOT, "_attic")
ISO = os.path.join(ROOT, "varix-qemu.iso")
TEST_IMG = os.path.join(ATTIC, "kbd-testdisk.img")
SHARED_IMG = os.path.join(ATTIC, "kbd-shared-exfat.img")
SERIAL = os.path.join(ATTIC, "qemu-kbd-serial.log")
SHOT_DIR = os.path.join(ATTIC, "acceptance-kbd")
MON_PORT = 14733

BOOT_TIMEOUT = 600
STEP_TIMEOUT = 60


def sh(cmd):
    print("+", " ".join(cmd))
    return subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)


def make_disks():
    with open(TEST_IMG, "wb") as f:
        f.truncate(1_048_576 * 512)
        f.seek(60_000 * 512)
        f.write(b"KBD-PROBE".ljust(512, b"0"))
    r = sh([sys.executable, os.path.join(ATTIC, "mkexfat.py"), SHARED_IMG])
    if r.returncode != 0:
        print(r.stdout, r.stderr)
        raise SystemExit("mkexfat failed")
    os.makedirs(SHOT_DIR, exist_ok=True)


def read_log():
    try:
        with open(SERIAL, "r", errors="replace") as f:
            return f.read()
    except OSError:
        return ""


class Mon:
    def __init__(self, port):
        self.s = socket.create_connection(("127.0.0.1", port), timeout=10)
        time.sleep(0.5)
        self.s.recv(65536)

    def cmd(self, c):
        self.s.sendall(c.encode() + b"\n")
        time.sleep(0.6)
        try:
            return self.s.recv(65536).decode(errors="replace")
        except Exception:
            return ""

    def key(self, k):
        self.cmd("sendkey " + k)

    def shot(self, name):
        self.cmd("screendump " + os.path.join(SHOT_DIR, name))


def wait_marker(marker, timeout=STEP_TIMEOUT, bad=None):
    t0 = time.time()
    deadline = t0 + timeout
    while time.time() < deadline:
        log = read_log()
        if marker in log:
            print(f"  [{time.time()-t0:6.1f}s] marker: {marker}")
            return True
        if bad and any(b in log for b in bad):
            print("BAD MARKER in log:", [b for b in bad if b in log])
            return False
        time.sleep(0.05)
    return False


def main():
    make_disks()
    if not os.path.exists(ISO):
        raise SystemExit("missing ISO: " + ISO + " (先跑 python scripts/make-iso-qemu.py)")
    if os.path.exists(SERIAL):
        os.remove(SERIAL)
    proc = subprocess.Popen(
        [
            "qemu-system-x86_64",
            "-machine", "q35",
            "-cdrom", ISO,
            "-drive", f"file={TEST_IMG},if=none,id=nv1,format=raw",
            "-device", "nvme,drive=nv1,serial=KBDA",
            "-drive", f"file={SHARED_IMG},if=none,id=nv2,format=raw",
            "-device", "nvme,drive=nv2,serial=KBDB",
            "-serial", "file:" + SERIAL,
            "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait",
            "-m", "1024",
        ],
        cwd=ROOT,
    )
    checks = []
    try:
        # A. controller_init 已跑（探针字上串口）
        ok = wait_marker("kbd-init: probe=", BOOT_TIMEOUT,
                         bad=("kernel panic", "triple fault"))
        checks.append(("kbd-init probe", ok))
        log = read_log()
        probe_line = [l for l in log.splitlines() if "kbd-init: probe=" in l]
        print("  probe line:", probe_line[-1] if probe_line else "MISSING")

        # B. 注入三键+鼠标 → 探针窗口提前退出（控制器初始化后路径零回归）
        mon = Mon(MON_PORT)
        wait_marker("input-probe: live", BOOT_TIMEOUT)
        mon.key("up"); time.sleep(0.3)
        mon.key("down"); time.sleep(0.3)
        mon.key("ret"); time.sleep(0.3)
        mon.cmd("mouse_move 60 40"); time.sleep(0.3)
        mon.cmd("mouse_move 120 80"); time.sleep(0.3)
        mon.cmd("mouse_button 1"); time.sleep(0.3)
        mon.cmd("mouse_button 0"); time.sleep(0.3)
        ok = wait_marker("input-probe: verdict=true", 150)
        checks.append(("probe verdict=true (kbd path alive)", ok))

        # C. BootScreen 全程不按键 → 10s 超时自动进桌面（兜底路径）
        ok = wait_marker("SHELL: boot-replay done", BOOT_TIMEOUT)
        checks.append(("boot-replay done", ok))
        time.sleep(3.0)   # 诊断行/倒计时首帧已画
        mon.shot("01-bootscreen-diag-t7.png")
        time.sleep(5.0)   # 倒计时继续走
        mon.shot("02-bootscreen-diag-t2.png")
        ok = wait_marker("SHELL: desktop-ready", 20)
        checks.append(("desktop-ready WITHOUT any key (10s auto-continue)", ok))

        # D. 桌面按键路径活着
        time.sleep(0.8)
        mon.key("spc")
        ok = wait_marker("SHELL: startmenu opened", STEP_TIMEOUT)
        checks.append(("startmenu opened (desktop key path)", ok))
        time.sleep(0.6)
        mon.shot("03-startmenu-after-auto-continue.png")

        # F. RESTART：菜单第四项 → SYS_REBOOT → 复位 → 整机真重启（第二次
        #    boot 链出现在同一串口日志=复位送达；无 -no-reboot，QEMU 走真重启）。
        for _ in range(3):
            mon.key("down")
            time.sleep(0.3)
        mon.shot("04-startmenu-restart-selected.png")
        mon.key("ret")
        ok = wait_marker("SHELL: restart requested", STEP_TIMEOUT)
        checks.append(("restart requested (SYS_REBOOT dispatched)", ok))
        ok2 = wait_marker("reboot: 8042 pulse", STEP_TIMEOUT)
        checks.append(("kernel reboot ladder entered (8042 pulse)", ok2))
        ok3 = False
        t0 = time.time()
        while time.time() - t0 < 120:
            if read_log().count("kbd-init: probe=") >= 2:
                ok3 = True
                break
            time.sleep(0.2)
        checks.append(("machine reset delivered (second boot in serial)", ok3))
        time.sleep(3.0)
        mon.shot("05-second-boot-bootscreen.png")

        # 汇总
        log = read_log()
        print("\n=== KBD SMOKE CHECKS ===")
        all_ok = True
        for name, ok in checks:
            print(f"  {name}: {'PASS' if ok else 'FAIL'}")
            all_ok = all_ok and ok
        for m in ["kbd-init: probe=", "SHELL: boot-replay done",
                  "SHELL: desktop-ready", "SHELL: startmenu opened",
                  "SHELL: restart requested", "reboot: SYS_REBOOT",
                  "reboot: 8042 pulse"]:
            hit = m in log
            print(f"  serial[{m}]: {'PASS' if hit else 'FAIL'}")
            all_ok = all_ok and hit
        bad = [b for b in ("kernel panic", "triple fault") if b in log]
        print("  bad-markers:", bad or "none")
        all_ok = all_ok and not bad
        print("\nKBD SMOKE VERDICT:", "PASS" if all_ok else "FAIL")
        return 0 if all_ok else 1
    finally:
        try:
            proc.kill()
        except Exception:
            pass


if __name__ == "__main__":
    raise SystemExit(main())
