#!/usr/bin/env python3
"""任务58（AI-P）· 拔出全链演练：运行中强拔 → 中断 → 下次插入恢复。

链路（QEMU 等价先行；物理强拔的控制器错误层级差异已由任务10 三阶段
×9 轮如实登记，实机物理拔出补验登记为待用户协作项）：

  会话1：引导 → 桌面 → 文件管理器 SHARED exFAT 真实数据源
         （source=shared-exfat）→ 设置页 RAM KV 写入 rc=0。
  运行中强拔：HMP drive_del/device_del 拔除 SHARED 盘（q35 根复合体
         直挂设备热拔插需 guest ACPI 配合——本内核无弹出处理，如实
         登记；后端移除路径走 HMP 响应与内核 IO 失效检测双取证）→
         重新进入文件管理器 → 内核「IO 失效卸载」触发（SHARED io
         failed - uninstalled）→ 数据源如实降级 demo-tree（零冒充）
         → 全程零 panic/fatal。
  会话2（断电+重插恢复）：kill QEMU（断电）→ 重建同内容 SHARED 盘 →
         重新引导 → SHARED 恢复挂载 source=shared-exfat → KV 重新
         写入 rc=0（RAM KV .bss 后端会话级语义 = 断电即失，页脚
         「STORE: KV-RAM (session)」已如实公示；重插后写入即干净重建）。

证据：_attic/p58-serial-{s1,s2}.log + _attic/p58-drill-report.json
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
S1 = os.path.join(ATTIC, "p58-serial-s1.log")
S2 = os.path.join(ATTIC, "p58-serial-s2.log")
REPORT = os.path.join(ATTIC, "p58-drill-report.json")
MON_PORT = 14731

BOOT_TIMEOUT = 900
STEP_TIMEOUT = 30
BAD = ("kernel panic", "VERDICT=FAIL", "verdict=FAIL", "triple fault", "#DF")


def sh(cmd):
    print("+", " ".join(cmd))
    return subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)


def make_shared():
    r = sh([sys.executable, os.path.join(ATTIC, "mkexfat.py"), SHARED_IMG])
    if r.returncode != 0:
        print(r.stdout, r.stderr)
        raise SystemExit("mkexfat failed")


def read_log(path):
    try:
        with open(path, "r", errors="replace") as f:
            return f.read()
    except OSError:
        return ""


class Mon:
    """HMP 客户端：原始 socket 收发（makefile+超时会毒化缓冲——实测）。"""

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
        self._drain(1.0)  # 吃掉 banner

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

    def quit(self):
        try:
            self.s.sendall(b"quit\n")
        except OSError:
            pass


def wait_marker(path, marker, timeout=STEP_TIMEOUT, since=0, bad=None):
    """since=字节偏移：只认该偏移之后的新里程碑。命中返回当前日志长度。"""
    t0 = time.time()
    while time.time() - t0 < timeout:
        log = read_log(path)
        tail = log[since:]
        if marker in tail:
            return len(log)
        if bad and any(b in tail for b in bad):
            print("BAD MARKER:", [b for b in bad if b in tail])
            return -1
        time.sleep(0.05)
    return -1


def qemu_args(serial):
    return [
        "qemu-system-x86_64",
        "-machine", "q35",
        "-cdrom", ISO,
        "-drive", f"file={TEST_IMG},if=none,id=nv1,format=raw",
        "-device", "nvme,drive=nv1,id=nv1dev,serial=P58A",
        "-drive", f"file={SHARED_IMG},if=none,id=nv2,format=raw",
        "-device", "nvme,drive=nv2,id=nv2dev,serial=P58B",
        "-serial", "file:" + serial,
        "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait",
        "-m", "1024",
        "-no-reboot", "-no-shutdown",
    ]


def kill(proc):
    try:
        proc.kill()
    except OSError:
        pass


def boot_to_desktop(serial, port):
    """引导 → 任意键 → 桌面。ushell 在 boot-replay done 后必须先收任意键
    才进桌面——先发 spc 再等 desktop-ready（否则双向死等）。"""
    if os.path.exists(serial):
        os.remove(serial)
    proc = subprocess.Popen(qemu_args(serial), cwd=ROOT)
    o = wait_marker(serial, "SHELL: boot-replay done", BOOT_TIMEOUT,
                    bad=("kernel panic", "triple fault"))
    if o < 0:
        kill(proc)
        raise SystemExit(f"boot failed: no boot-replay in {serial}")
    time.sleep(1.0)
    mon = Mon(port)
    mon.key("spc")
    o = wait_marker(serial, "SHELL: desktop-ready", STEP_TIMEOUT, since=o)
    if o < 0:
        kill(proc)
        raise SystemExit(f"boot failed: no desktop-ready in {serial}")
    if "source=shared-exfat" not in read_log(serial):
        kill(proc)
        raise SystemExit(f"boot failed: SHARED not mounted in {serial}")
    return proc, mon, o


def goto_files(mon, serial, since):
    """桌面 → 开始菜单 → 文件管理器（files.refresh + 数据源重报）。"""
    mon.key("ret")
    o = wait_marker(serial, "SHELL: startmenu opened", STEP_TIMEOUT, since=since)
    if o < 0:
        return -1
    mon.key("ret")
    o = wait_marker(serial, "SHELL: files opened", STEP_TIMEOUT, since=o)
    if o < 0:
        return -1
    time.sleep(0.6)
    return o


def kv_set_show_menu(mon, serial, since, tries=2):
    """（可能处于文件页）ESC→桌面→设置→↓→Enter 切换 show_menu→KV 打点。"""
    for t in range(tries):
        mon.key("esc")
        time.sleep(0.5)
        mon.key("ret")
        o = wait_marker(serial, "SHELL: startmenu opened", STEP_TIMEOUT, since=since)
        if o < 0:
            continue
        mon.key("down")
        time.sleep(0.3)
        mon.key("ret")
        o = wait_marker(serial, "SHELL: settings opened", STEP_TIMEOUT, since=o)
        if o < 0:
            since = o if o > 0 else since
            continue
        mon.key("down")
        time.sleep(0.3)
        mon.key("ret")
        o = wait_marker(serial, "SHELL: settings set key=show_menu rc=0",
                        STEP_TIMEOUT, since=o)
        if o >= 0:
            mon.key("esc")
            time.sleep(0.5)
            return o
        since = len(read_log(serial))
    return -1


def main():
    checks = []
    mon_log = {}
    proc1 = proc2 = None
    mon1 = mon2 = None
    r_drive = r_dev = ""
    removal_refused = False
    try:
        make_shared()
        if not os.path.exists(TEST_IMG):
            with open(TEST_IMG, "wb") as f:
                f.truncate(1_048_576 * 512)
                f.seek(60_000 * 512)
                f.write(b"P2728-PROBE".ljust(512, b"0"))

        # ---------------- 会话 1：SHARED 在位，KV 写入成功 ----------------
        proc1, mon1, o = boot_to_desktop(S1, MON_PORT)
        checks.append(("s1: boot + desktop + SHARED mounted", True))
        o = goto_files(mon1, S1, o)
        if o < 0:
            raise SystemExit("s1: files not opened")
        checks.append(("s1: fm via files.refresh (source re-report)", True))
        o = kv_set_show_menu(mon1, S1, o)
        checks.append(("s1: kv_set show_menu rc=0", o >= 0))
        if o < 0:
            raise SystemExit("s1: kv_set failed")

        # ---------------- 运行中强拔 SHARED ----------------
        mark = len(read_log(S1))
        r_drive = mon1.cmd("drive_del nv2", settle=0.6)
        r_dev = mon1.cmd("device_del nv2dev", settle=0.6)
        print("  [monitor] drive_del nv2       ->",
              (r_drive.strip().splitlines() or ["?"])[-1][:80])
        print("  [monitor] device_del nv2dev   ->",
              (r_dev.strip().splitlines() or ["?"])[-1][:80])
        removal_refused = "in use" in (r_drive + r_dev).lower()
        time.sleep(2.0)
        # 使用点移除检测：重进文件管理器 → 内核 IO 失效卸载 → 如实降级。
        o = goto_files(mon1, S1, mark)
        log1 = read_log(S1)[mark:]
        uninstalled = "SHARED io failed - uninstalled" in log1
        degraded = "source=demo-tree" in log1
        checks.append(("pull: kernel IO-failure uninstall", uninstalled))
        checks.append(("pull: fm degrade to demo-tree (honest)", degraded))
        o = kv_set_show_menu(mon1, S1, o)  # 起点=文件页标记之后（mark 会误中旧标记）
        checks.append(("pull: kv_set rc=0 after removal (RAM KV independent)", o >= 0))
        bad1 = [b for b in BAD if b in read_log(S1)[mark:]]
        checks.append(("pull: no panic/fatal after removal", not bad1))
        if bad1:
            print("  BAD:", bad1)

        # ---------------- 会话 2：断电 + 重插恢复 ----------------
        mon1.quit()
        time.sleep(1.0)
        kill(proc1)
        proc1 = None
        make_shared()  # 重插（同内容重建）
        proc2, mon2, o = boot_to_desktop(S2, MON_PORT)
        checks.append(("replug: SHARED re-mounted (source=shared-exfat)", True))
        o = goto_files(mon2, S2, o)
        if o < 0:
            raise SystemExit("replug: files not opened")
        o = kv_set_show_menu(mon2, S2, o)
        checks.append(("replug: kv_set rc=0 (clean rebuild)", o >= 0))
        bad2 = [b for b in BAD if b in read_log(S2)]
        checks.append(("replug: no panic/fatal", not bad2))
    except SystemExit as e:
        checks.append((f"aborted: {e}", False))
    finally:
        if mon2:
            mon2.quit()
        if mon1:
            mon1.quit()
        time.sleep(0.8)
        if proc2:
            kill(proc2)
        if proc1:
            kill(proc1)

    print("\n=== 任务58 · 拔出全链演练 ===")
    all_ok = True
    for name, ok in checks:
        print(f"  {name}: {'PASS' if ok else 'FAIL'}")
        all_ok = all_ok and ok
    verdict = "PASS" if all_ok else "FAIL"
    print("PULL-CHAIN DRILL:", verdict)
    with open(REPORT, "w", encoding="utf-8") as f:
        json.dump({
            "verdict": verdict,
            "checks": checks,
            "monitor": {
                "drive_del_nv2": (r_drive.strip().splitlines() or ["?"])[-1][:120],
                "device_del_nv2dev": (r_dev.strip().splitlines() or ["?"])[-1][:120],
                "removal_refused_guest_no_acpi": removal_refused,
                "note": "q35 热拔插需 guest ACPI 配合；介质移除经由后端删除路径，"
                        "内核以 IO 失效卸载感知（用户可见=数据源如实降级 demo-tree）。",
            },
        }, f, ensure_ascii=False, indent=1)
    return 0 if all_ok else 1


if __name__ == "__main__":
    sys.exit(main())
