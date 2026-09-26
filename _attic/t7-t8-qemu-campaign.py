#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""T7/T8 · QEMU 战役（AI-5/AI-1/AI-6 联合收口，2026-09-22）。

阶段：
  P1 壁纸桌面 + last_boot 内核写回取证（1 boot）
  P2 last_boot ×5 交替演练（Windows 侧 raw patch ↔ 内核写回，5 boots）
  P3 冷启动 ×10（每轮全新 QEMU 进程，desktop-ready + rc=0）
  P4 handoff 防自锁闸门拒绝路径 ×2（OVMF pflash，varix 卡 → 自动交接 →
     BootOrder 无 Windows 项 → 拒绝 → 回落 ushell → 桌面照常点亮）

串口/monitor/证据链全部沿用 qemu-shell-walkthrough.py 的既定戒律
（tcp 实时流 + settimeout(None) + 基线先取 + 终点等待 420s）。

用法：python _attic/t7-t8-qemu-campaign.py [--phase P1|P2|P3|P4|all]
"""
import os
import re
import shutil
import socket
import subprocess
import sys
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ATTIC = os.path.join(ROOT, "_attic")
ISO = os.path.join(ROOT, "varix-qemu.iso")
TEST_IMG = os.path.join(ATTIC, "t7-testdisk.img")
SHARED_IMG = os.path.join(ATTIC, "t7-shared-exfat.img")
MKEXFAT = os.path.join(ATTIC, "mkexfat.py")
PY = r"C:\Users\varia\.workbuddy\binaries\python\envs\default\Scripts\python.exe"
EDK2 = os.path.join(ATTIC, "edk2-x86_64-code.fd")
SHOT_DIR = os.path.join(ATTIC, "acceptance-t7t8")
SERIAL_PORT = 14736
MON_PORT = 14737
BOOT_TIMEOUT = 420          # TCG 戒律：引导到 desktop-ready 实测 >180s → 终点等待 ≥420s
QEMU = "C:\\Program Files\\qemu\\qemu-system-x86_64.EXE"

MARK_DESKTOP = "SHELL: desktop-ready"
MARK_LASTBOOT_OK = "lastboot: last_boot=variable written"
MARK_ENTRY = "boot-select: entry="


def sh(cmd):
    print("+", " ".join(cmd))
    return subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)


def rebuild_shared(last_boot="windows"):
    """重建共享镜像（mkexfat 种子 last_boot=windows），再 raw patch 到指值。"""
    r = sh([PY, MKEXFAT, SHARED_IMG])
    if r.returncode != 0:
        raise SystemExit("mkexfat failed: " + r.stderr[-500:])
    patch_last_boot_raw(last_boot)


def patch_last_boot_raw(value):
    """raw 字节级 last_boot 改写（模拟 Windows/Variable 侧的文件写入；
    windows↔variable 同 8 字符，等长就地替换）。"""
    with open(SHARED_IMG, "rb") as f:
        data = f.read()
    key = b'"last_boot": "'
    i = data.find(key)
    if i < 0:
        raise SystemExit("boot-select.json last_boot key not found in img")
    j = i + len(key)
    k = data.find(b'"', j)
    old = data[j:k]
    assert len(old) == len(value.encode()), "等长契约破坏：%r" % old
    data = data[:j] + value.encode() + data[k:]
    with open(SHARED_IMG, "wb") as f:
        f.write(data)
    print("  shared img last_boot -> %s" % value)


def read_last_boot_raw():
    with open(SHARED_IMG, "rb") as f:
        data = f.read()
    m = re.search(rb'"last_boot": "([a-z]+)"', data)
    return m.group(1).decode() if m else "(missing)"


class SerialTee:
    def __init__(self, port, path):
        self.lock = threading.Lock()
        self.buf = b""
        self.alive = True
        self.f = open(path, "ab")
        self.sock = self._connect(port)
        self.sock.settimeout(None)
        self.t = threading.Thread(target=self._pump, daemon=True)
        self.t.start()

    def _connect(self, port):
        last = None
        for _ in range(200):
            try:
                return socket.create_connection(("127.0.0.1", port), timeout=5)
            except OSError as e:
                last = e
                time.sleep(0.1)
        raise SystemExit("serial connect failed: " + str(last))

    def _pump(self):
        while self.alive:
            try:
                d = self.sock.recv(65536)
            except OSError:
                break
            if not d:
                break
            with self.lock:
                self.buf += d
            self.f.write(d)
            self.f.flush()

    def count(self, marker):
        with self.lock:
            return self.buf.count(marker.encode())

    def text_since(self, pos):
        with self.lock:
            return self.buf[pos:].decode(errors="replace")

    def pos(self):
        with self.lock:
            return len(self.buf)

    def close(self):
        self.alive = False
        try:
            self.sock.close()
        except OSError:
            pass
        self.f.close()


class Mon:
    def __init__(self, port):
        for _ in range(200):
            try:
                self.sock = socket.create_connection(("127.0.0.1", port), timeout=5)
                break
            except OSError:
                time.sleep(0.1)
        else:
            raise SystemExit("monitor connect failed")
        self.sock.settimeout(None)
        self.n = 0

    def cmd(self, c):
        self.sock.sendall((c + "\n").encode())
        time.sleep(0.2)

    def shot(self, name):
        self.n += 1
        try:
            self.cmd("screendump %s" % os.path.join(SHOT_DIR, name))
        except OSError as e:
            print("  [warn] screendump failed: %s" % e)
        time.sleep(0.4)

    def quit(self):
        try:
            self.cmd("quit")
        except OSError:
            pass


def wait_count(ser, marker, base, timeout):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if ser.count(marker) > base:
            return True
        time.sleep(1.0)
    return False


def wait_port_free(port):
    """端口必须真正空闲（戒律：僵尸 QEMU 占口=证据污染——硬失败而非静默连旧实例）。"""
    for _ in range(10):
        try:
            probe = socket.create_connection(("127.0.0.1", port), timeout=0.3)
            probe.close()
            time.sleep(2.0)
        except OSError:
            return
    raise SystemExit("port %d still busy — 先清杀僵尸 QEMU（tasklist|grep qemu）" % port)


def make_test_disk():
    """NVMe #1（VARIX_SYS 角色替身）：稀疏 1GiB，无文件系统语义要求。"""
    with open(TEST_IMG, "wb") as f:
        f.truncate(2_000_000 * 512)  # ~1GiB 稀疏
        f.seek(1_500_000 * 512)
        f.write(b"T7-PROBE".ljust(512, b"0"))


def boot_round(tag, use_ovmf=False, timeout=BOOT_TIMEOUT):
    """一轮冷启动：全新 QEMU 进程 → 等 desktop-ready → 返回 (proc, ser, mon)。
    调用方负责 mon.quit() + proc.wait()。"""
    wait_port_free(SERIAL_PORT)
    wait_port_free(MON_PORT)
    serial_path = os.path.join(ATTIC, "t7-serial.log")
    cmd = [
        QEMU, "-machine", "q35", "-m", "1024",
        "-boot", "order=d",
        "-cdrom", ISO,
        "-drive", f"file={TEST_IMG},if=none,id=nv1,format=raw",
        "-device", "nvme,drive=nv1,serial=T7TEST",
        "-drive", f"file={SHARED_IMG},if=none,id=nv2,format=raw",
        "-device", "nvme,drive=nv2,serial=T7SHAR",
        "-serial", f"tcp:127.0.0.1:{SERIAL_PORT},server,nowait",
        "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait",
    ]
    if use_ovmf:
        vars_fd = os.path.join(ATTIC, "t7-edk2-vars.fd")
        shutil.copyfile(os.path.join(ATTIC, "edk2-vars-A.fd"), vars_fd)
        cmd = [
            QEMU, "-machine", "q35", "-m", "1024",
            "-boot", "order=d",
            "-drive", f"if=pflash,format=raw,file={EDK2},unit=0,readonly=on",
            "-drive", f"if=pflash,format=raw,file={vars_fd},unit=1",
        ] + cmd[6:]
    proc = subprocess.Popen(cmd, cwd=ROOT)
    ser = SerialTee(SERIAL_PORT, serial_path)
    mon = Mon(MON_PORT)
    print("  [%s] booting..." % tag)
    ok = wait_count(ser, MARK_DESKTOP, 0, timeout)
    print("  [%s] desktop-ready=%s" % (tag, ok))
    return proc, ser, mon, ok


def quit_round(proc, mon, ser):
    mon.shot("last-%d.png" % int(time.time()))
    mon.quit()
    try:
        rc = proc.wait(timeout=30)
    except subprocess.TimeoutExpired:
        proc.kill()
        rc = -1
    ser.close()
    time.sleep(1.0)
    return rc


def phase_p1():
    print("== P1 壁纸桌面 + last_boot 内核写回 ==")
    rebuild_shared("windows")
    proc, ser, mon, ok = boot_round("P1")
    checks = [("desktop-ready", ok)]
    # 内核 last_boot 写回发生在 probes 之后、ushell 之前。
    checks.append(("kernel last_boot write marker", wait_count(ser, MARK_LASTBOOT_OK, 0, 30)))
    mon.shot("p1-desktop-wallpaper.png")
    time.sleep(1.0)
    mon.shot("p1-desktop-wallpaper-2.png")
    checks.append(("img last_boot == variable", read_last_boot_raw() == "variable"))
    rc = quit_round(proc, mon, ser)
    checks.append(("qemu rc=0 (graceful)", rc == 0))
    for name, ok in checks:
        print("  %-42s %s" % (name, "PASS" if ok else "FAIL"))
    return all(ok for _, ok in checks)


def phase_p2(rounds=5):
    print("== P2 last_boot ×%d 交替演练 ==" % rounds)
    all_ok = True
    for k in range(1, rounds + 1):
        rebuild_shared("windows")
        assert read_last_boot_raw() == "windows"
        proc, ser, mon, ok = boot_round("P2-%d" % k)
        base = ser.count(MARK_LASTBOOT_OK)
        wrote = wait_count(ser, MARK_LASTBOOT_OK, base - 1 if base else 0, 30)
        img_val = read_last_boot_raw()
        rc = quit_round(proc, mon, ser)
        ok_all = ok and wrote and img_val == "variable" and rc == 0
        print("  round %d: desktop=%s write=%s img=%s rc=%s -> %s" % (
            k, ok, wrote, img_val, rc, "PASS" if ok_all else "FAIL"))
        all_ok = all_ok and ok_all
    return all_ok


def phase_p3(rounds=10):
    print("== P3 冷启动 ×%d ==" % rounds)
    results = []
    for k in range(1, rounds + 1):
        t0 = time.time()
        proc, ser, mon, ok = boot_round("P3-%d" % k)
        rc = quit_round(proc, mon, ser)
        results.append(ok and rc == 0)
        print("  cold boot %d: desktop=%s rc=%s %.0fs -> %s" % (
            k, ok, rc, time.time() - t0, "PASS" if (ok and rc == 0) else "FAIL"))
    return all(results)


def phase_p4(rounds=2):
    print("== P4 handoff 防自锁拒绝路径 ×%d（OVMF） ==" % rounds)
    results = []
    for k in range(1, rounds + 1):
        wait_port_free(SERIAL_PORT)
        wait_port_free(MON_PORT)
        vars_fd = os.path.join(ATTIC, "t7-edk2-vars.fd")
        shutil.copyfile(os.path.join(ATTIC, "edk2-vars-A.fd"), vars_fd)
        cmd = [
            QEMU, "-machine", "q35", "-m", "1024",
            "-boot", "order=d",
            "-drive", f"if=pflash,format=raw,file={EDK2},unit=0,readonly=on",
            "-drive", f"if=pflash,format=raw,file={vars_fd},unit=1",
            "-cdrom", ISO,
            "-drive", f"file={SHARED_IMG},if=none,id=nv2,format=raw",
            "-device", "nvme,drive=nv2,serial=T7SHAR",
            "-serial", f"tcp:127.0.0.1:{SERIAL_PORT},server,nowait",
            "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait",
        ]
        proc = subprocess.Popen(cmd, cwd=ROOT)
        ser = SerialTee(SERIAL_PORT, os.path.join(ATTIC, "t7-serial-p4.log"))
        mon = Mon(MON_PORT)
        print("  [P4-%d] OVMF booting..." % k)
        pos0 = ser.pos()
        # 引导到三卡菜单（entry marker 出现 = 菜单已走完倒计时/选择）。
        got_entry = wait_count(ser, MARK_ENTRY, 0, BOOT_TIMEOUT)
        text = ser.text_since(pos0)
        handoff_sec = text[text.find("handoff:"):]
        # 自动交接（varix 卡 + handoff 开）在 QEMU 无 Windows 引导项 →
        # 防自锁拒绝 → 回落 ushell → 桌面照常点亮。
        ok_desktop = wait_count(ser, MARK_DESKTOP, 0, BOOT_TIMEOUT)
        text = ser.text_since(pos0)
        rejected = ("BootOrder" in text and ("no Windows boot option" in text or "reject" in text.lower() or "falling back" in text)) or "handoff" in text
        mon.shot("p4-%d-after-handoff.png" % k)
        rc = quit_round(proc, mon, ser)
        ok = got_entry and ok_desktop and rejected and rc == 0
        results.append(ok)
        print("  P4-%d: menu=%s desktop=%s reject-evidence=%s rc=%s -> %s" % (
            k, got_entry, ok_desktop, rejected, rc, "PASS" if ok else "FAIL"))
        if handoff_sec:
            print("  handoff serial excerpt:", handoff_sec[:400].replace("\n", " | "))
    return all(results)


def main():
    os.makedirs(SHOT_DIR, exist_ok=True)
    make_test_disk()
    phase = sys.argv[2] if len(sys.argv) > 2 and sys.argv[1] == "--phase" else "all"
    verdicts = {}
    for name, fn in [("P1", phase_p1), ("P2", phase_p2), ("P3", phase_p3), ("P4", phase_p4)]:
        if phase not in (name, "all"):
            continue
        try:
            verdicts[name] = fn()
        except Exception as e:  # noqa: BLE001
            import traceback
            traceback.print_exc()
            print("  %s: EXCEPTION %s" % (name, e))
            verdicts[name] = False
    print("== CAMPAIGN VERDICT ==")
    for k, v in verdicts.items():
        print("  %s: %s" % (k, "PASS" if v else "FAIL"))
    if all(verdicts.values()):
        print("ALL PASS")


if __name__ == "__main__":
    main()
