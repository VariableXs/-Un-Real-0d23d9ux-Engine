#!/usr/bin/env python3
"""任务27/28/55（AI-V）· 内核嵌入层实机走查驱动。

旅程：BootScreen 回放 → 桌面 → 开始菜单 → 文件管理器（SHARED exFAT 真实
列表+读文件预览）→ 设置页（KV 读写）→ 关于。全程键盘（HMP sendkey），
逐屏 screendump 归档，串口里程碑逐条断言。

用法：python _attic/p27-28-shell-demo.py
证据：_attic/p27-28-serial.log + _attic/acceptance-p27/*.png
"""
import os
import socket
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ATTIC = os.path.join(ROOT, "_attic")
# 走既有验证链路：scripts/make-iso-qemu.py → varix-qemu.iso（p42-45/47/56 实机同款）。
# build-iso.py 的 varix.iso 是 AURORA-1000 旧链路，当前源码树下 Limine iso9660 读
# 内核数据 PANIC（iso9660: failed to read file data），实测两次复现，勿再回切。
ISO = os.path.join(ROOT, "varix-qemu.iso")
TEST_IMG = os.path.join(ATTIC, "p27-testdisk.img")
SHARED_IMG = os.path.join(ATTIC, "p27-shared-exfat.img")
SERIAL = os.path.join(ATTIC, "p27-28-serial.log")
SHOT_DIR = os.path.join(ATTIC, "acceptance-p27")
MON_PORT = 14721

BOOT_TIMEOUT = 900
STEP_TIMEOUT = 30
PERF = {}  # 任务71：里程碑耗时采样（perf-gate-usb.py 消费）


def sh(cmd):
    print("+", " ".join(cmd))
    return subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)


def make_disks():
    with open(TEST_IMG, "wb") as f:
        f.truncate(1_048_576 * 512)
        f.seek(60_000 * 512)
        f.write(b"P2728-PROBE".ljust(512, b"0"))
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
        time.sleep(0.8)
        try:
            return self.s.recv(65536).decode(errors="replace")
        except Exception:
            return ""

    def key(self, k):
        self.cmd("sendkey " + k)

    def shot(self, name):
        self.cmd("screendump " + os.path.join(SHOT_DIR, name))


def wait_marker(mon, marker, timeout=STEP_TIMEOUT, bad=None):
    # 任务71：命中即记录本步骤耗时（ms）；0.05s 轮询让交互 P95 可测
    #（0.4s 粒度下 P95<100ms 阈值在物理上不可满足）。
    t0 = time.time()
    deadline = t0 + timeout
    while time.time() < deadline:
        log = read_log()
        if marker in log:
            PERF.setdefault("steps", []).append(
                {"marker": marker, "ms": round((time.time() - t0) * 1000, 1)})
            return True
        if bad and any(b in log for b in bad):
            print("BAD MARKER in log:", [b for b in bad if b in log])
            return False
        time.sleep(0.05)
    return False


def main():
    make_disks()
    if not os.path.exists(ISO):
        raise SystemExit("missing ISO: " + ISO + " (先跑 python tools/build-iso.py)")
    if os.path.exists(SERIAL):
        os.remove(SERIAL)
    mon_sock = None
    proc = subprocess.Popen(
        [
            "qemu-system-x86_64",
            "-machine", "q35",
            "-cdrom", ISO,
            "-drive", f"file={TEST_IMG},if=none,id=nv1,format=raw",
            "-device", "nvme,drive=nv1,serial=P2728A",
            "-drive", f"file={SHARED_IMG},if=none,id=nv2,format=raw",
            "-device", "nvme,drive=nv2,serial=P2728B",
            "-serial", "file:" + SERIAL,
            "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait",
            "-m", "1024",
            "-no-reboot", "-no-shutdown",
        ],
        cwd=ROOT,
    )
    checks = []
    try:
        # ① BootScreen 回放
        ok = wait_marker(None, "SHELL: boot-replay done", BOOT_TIMEOUT,
                         bad=("kernel panic", "fatal"))
        checks.append(("boot-replay", ok))
        if not ok:
            raise RuntimeError("boot-replay marker missing")
        time.sleep(1.0)
        mon = Mon(MON_PORT)
        mon.shot("01-bootscreen.png")

        # ② 任意键 → 桌面（三件套就绪 + 文件列表打点）
        mon.key("spc")
        ok = wait_marker(None, "SHELL: desktop-ready", STEP_TIMEOUT)
        checks.append(("desktop-ready", ok))
        ok2 = wait_marker(None, "SHELL: fm count=", STEP_TIMEOUT)
        checks.append(("fm-listed", ok2))
        time.sleep(0.6)
        mon.shot("02-desktop.png")

        # ③ 开始菜单
        mon.key("ret")
        ok = wait_marker(None, "SHELL: startmenu opened", STEP_TIMEOUT)
        checks.append(("startmenu", ok))
        time.sleep(0.6)
        mon.shot("03-startmenu.png")

        # ④ 文件管理器（菜单第一项 Files）
        mon.key("ret")
        ok = wait_marker(None, "SHELL: files opened", STEP_TIMEOUT)
        checks.append(("files-opened", ok))
        time.sleep(0.8)
        mon.shot("04-files.png")

        # ⑤ 打开 apps.json（根目录第 5 项：↓×4 + Enter；逐次重试直到打点）
        opened = False
        for _ in range(8):
            mon.key("ret")
            if wait_marker(None, "SHELL: file opened name=", 6):
                opened = True
                break
            mon.key("down")
            time.sleep(0.3)
        checks.append(("file-opened", opened))
        time.sleep(0.6)
        mon.shot("05-fileview.png")

        # ⑥ 返回列表 → Esc 回桌面 → 开始菜单 → 设置
        mon.key("esc")
        time.sleep(0.4)
        mon.key("esc")
        time.sleep(0.4)
        mon.key("ret")
        wait_marker(None, "SHELL: startmenu opened", STEP_TIMEOUT)
        mon.key("down")
        time.sleep(0.3)
        mon.key("ret")
        ok = wait_marker(None, "SHELL: settings opened", STEP_TIMEOUT)
        checks.append(("settings-opened", ok))
        time.sleep(0.6)
        mon.shot("06-settings.png")

        # ⑦ 引导超时 ←→ 调整（KV 写入打点）+ 截图
        mon.key("right")
        ok = wait_marker(None, "SHELL: settings set key=boot_timeout rc=0", STEP_TIMEOUT)
        checks.append(("kv-set-timeout", ok))
        time.sleep(0.5)
        mon.shot("07-settings-changed.png")

        # ⑧ Esc → START → down×2 → About
        mon.key("esc")
        time.sleep(0.4)
        mon.key("ret")
        wait_marker(None, "SHELL: startmenu opened", STEP_TIMEOUT)
        mon.key("down")
        mon.key("down")
        time.sleep(0.3)
        mon.key("ret")
        ok = wait_marker(None, "SHELL: about opened", STEP_TIMEOUT)
        checks.append(("about-opened", ok))
        time.sleep(0.6)
        mon.shot("08-about.png")

        # 汇总
        log = read_log()
        print("\n=== SERIALIZED CHECKS ===")
        all_ok = True
        for name, ok in checks:
            print(f"  {name}: {'PASS' if ok else 'FAIL'}")
            all_ok = all_ok and ok
        # 串口里程碑逐条复核
        for m in [
            "SHELL: first-frame ms=",
            "SHELL: boot-replay done",
            "SHELL: desktop-ready",
            "SHELL: fm count=",
            "source=shared-exfat",
            "SHELL: startmenu opened",
            "SHELL: files opened",
            "SHELL: file opened name=",
            "SHELL: settings opened",
            "SHELL: settings set key=boot_timeout rc=0",
            "SHELL: about opened",
        ]:
            hit = m in log
            print(f"  serial[{m}]: {'PASS' if hit else 'FAIL'}")
            all_ok = all_ok and hit
        bad = [b for b in ("kernel panic", "VERDICT=FAIL", "verdict=FAIL", "triple fault") if b in log]
        print("  bad-markers:", bad or "none")
        all_ok = all_ok and not bad
        print("\nSHELL DEMO VERDICT:", "PASS" if all_ok else "FAIL")
        # 任务71：boot ms + 首帧 ms（内核时钟实测，ushell 首画帧打点）落盘。
        PERF["boot_ms"] = None
        PERF["first_frame_ms"] = None
        import re as _re
        m = _re.search(r"boot completed (\d+) ms", log)
        if m:
            PERF["boot_ms"] = int(m.group(1))
        m2 = _re.search(r"SHELL: first-frame ms=(\d+)", log)
        if m2:
            PERF["first_frame_ms"] = int(m2.group(1))
        with open(os.path.join(ATTIC, "p27-28-perf.json"), "w", encoding="utf-8") as f:
            import json as _json
            _json.dump(PERF, f, ensure_ascii=False, indent=1)
        print("  perf-json:", os.path.join(ATTIC, "p27-28-perf.json"))
        return 0 if all_ok else 1
    finally:
        try:
            proc.kill()
        except Exception:
            pass


if __name__ == "__main__":
    raise SystemExit(main())
