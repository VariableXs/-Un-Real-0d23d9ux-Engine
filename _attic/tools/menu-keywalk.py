#!/usr/bin/env python3
r"""三卡菜单键盘走查（QEMU，BIOS/SeaBIOS 模式）——「任意键盘可自主选择系统」的
端到端证明（需求1/6）。

背景（实测得出，别踩）：
  * Limine 在 ISO9660/El Torito + OVMF 下报 "Could not meaningfully match
    the boot device handle" → 停在 "Press any key"，内核根本不启动。
    → 菜单/键盘走查用 BIOS 模式（正式走查 qemu-shell-walkthrough.py 同款）。
  * 串口在 socket 连接前的早期行会丢（cmdline/bootopt 行常看不到）——
    因此**视觉判据走 screendump**（菜单屏上的倒计时与高亮卡），不依赖早期串口行。
  * 菜单选中项退出后打的 `boot-select: entry=<id>` 是可选判据；windows/uefi
    两项在 BIOS 模式下会如实报告「无 UEFI Runtime Services」并继续起 varix，
    这正是我们要验证的**如实降级**语义。

场景（每步独立起一次 QEMU，互不污染）：
  1. baseline：不发键 → 默认项 varix（倒计时归零）
  2. down1    ：↓ 一次 → 高亮 WINDOWS；倒计时归零**不采纳**未确认预选（回落 varix）
  3. down1+enter：↓ 一次 + Enter → 选中 windows → 串口如实报告降级
  4. enter    ：不发 ↓，直接 Enter → 选中当前高亮（varix）
  5. down2    ：↓↓ 两次 → 高亮 FIRMWARE SETUP + Enter → 选中 uefi → 如实降级

用法：python _attic/tools/menu-keywalk.py [场景名...]   （不给则跑全部）
证据：_attic/acceptance-menu/keywalk/<场景>-*.png + keywalk-<场景>.log
"""
import os
import socket
import subprocess
import sys
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ATTIC = os.path.join(ROOT, "_attic")
ISO = os.path.join(ATTIC, "varix-qemu-menu.iso")
SHOT_DIR = os.path.join(ATTIC, "acceptance-menu", "keywalk")
QEMU = r"C:\Program Files\qemu\qemu-system-x86_64"

MON_PORT = 14831
SER_PORT = 14832
RUN_SECS = 130
NL = bytes([10])


class Pump(threading.Thread):
    """串口 TCP 实时流 → 缓冲 + 落盘。连接必须在 QEMU 起来之后。"""

    def __init__(self, port, path):
        super().__init__(daemon=True)
        self.buf = b""
        self.f = open(path, "wb")
        self.alive = True
        self.sock = None
        self.port = port

    def connect(self):
        for _ in range(200):
            try:
                self.sock = socket.create_connection(("127.0.0.1", self.port), timeout=5)
                break
            except OSError:
                time.sleep(0.1)
        if self.sock is not None:
            self.sock.settimeout(None)  # 超时传染戒律

    def run(self):
        if self.sock is None:
            return
        while self.alive:
            try:
                d = self.sock.recv(65536)
            except OSError:
                break
            if not d:
                break
            self.buf += d
            self.f.write(d)
            self.f.flush()

    def text(self):
        return self.buf.decode(errors="replace")


class Mon:
    def __init__(self, port):
        self.s = socket.create_connection(("127.0.0.1", port), timeout=10)
        time.sleep(0.4)
        try:
            self.s.recv(65536)
        except Exception:
            pass

    def cmd(self, c):
        try:
            self.s.sendall(c.encode() + NL)
        except OSError:
            return ""
        time.sleep(0.5)
        try:
            return self.s.recv(65536).decode(errors="replace")
        except Exception:
            return ""

    def key(self, k):
        self.cmd("sendkey " + k)

    def shot(self, name):
        os.makedirs(SHOT_DIR, exist_ok=True)
        self.cmd("screendump " + os.path.join(SHOT_DIR, name))

    def close(self):
        try:
            self.s.sendall(b"quit" + NL)
            time.sleep(0.5)
            self.s.close()
        except Exception:
            pass


def run_case_simple(name, down_presses, press_enter, shot_tags, key_delay=12.0, total=125):
    """线性脚本版（更直白、可复现）：t=key_delay 发键；shot_tags 指定时刻截图。"""
    ser_log = os.path.join(ATTIC, "acceptance-menu", f"keywalk-{name}.log")
    os.makedirs(os.path.dirname(ser_log), exist_ok=True)
    if os.path.exists(ser_log):
        os.remove(ser_log)
    pump = Pump(SER_PORT, ser_log)
    proc = subprocess.Popen(
        [
            QEMU, "-machine", "q35", "-m", "1024", "-smp", "2", "-cpu", "max",
            "-cdrom", ISO,
            "-boot", "order=d",
            "-serial", f"tcp:127.0.0.1:{SER_PORT},server,nowait",
            "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait",
            "-display", "none",
        ],
        cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT,
    )
    pump.connect()
    pump.start()
    print(f"[keywalk:{name}] QEMU pid={proc.pid}", flush=True)

    t0 = time.time()
    mon = None
    keys_sent = False
    made = []
    try:
        while time.time() - t0 < total:
            time.sleep(0.25)
            el = time.time() - t0
            if proc.poll() is not None:
                print(f"[keywalk:{name}] QEMU exited rc={proc.returncode}", flush=True)
                break
            if mon is None:
                try:
                    mon = Mon(MON_PORT)
                except OSError:
                    continue
            # 定时截图
            for at, tag in shot_tags:
                if tag not in made and el >= at:
                    mon.shot(f"{name}-{tag}.png")
                    made.append(tag)
                    print(f"[keywalk:{name}] shot t={el:.1f}s {tag}", flush=True)
            # 定时发键
            if not keys_sent and el >= key_delay:
                keys_sent = True
                for _ in range(down_presses):
                    mon.key("down")
                    time.sleep(0.8)
                if press_enter:
                    mon.key("ret")
                print(
                    f"[keywalk:{name}] keys sent at t={el:.1f}s: down x{down_presses}, "
                    f"enter={press_enter}",
                    flush=True,
                )
            if "boot-select: entry=" in pump.text():
                # 菜单已退出：留 6s 收尾截图后收工
                if el > key_delay + 8:
                    break
    finally:
        if mon is not None:
            mon.shot(f"{name}-final.png")
            made.append("final")
            mon.close()
        pump.alive = False
        try:
            proc.wait(timeout=8)
        except subprocess.TimeoutExpired:
            proc.kill()
    return pump.text(), made


CASES = {
    # name: (down 次数, 是否按 Enter, [(时刻, 标签)...])
    "1-baseline": (0, False, [(14, "menu"), (70, "after")]),
    "2-down1": (1, False, [(10, "menu"), (16, "after-down"), (70, "after")]),
    "3-down1-enter": (1, True, [(10, "menu"), (16, "after-down"), (22, "after-enter")]),
    "4-enter": (0, True, [(14, "menu"), (20, "after-enter")]),
    "5-down2-enter": (2, True, [(10, "menu"), (18, "after-down"), (24, "after-enter")]),
}


def main() -> int:
    which = sys.argv[1:] or list(CASES.keys())
    summary = []
    for name in which:
        if name not in CASES:
            print(f"未知场景 {name}", file=sys.stderr)
            return 1
        down, enter, shots = CASES[name]
        print(f"\n===== 场景 {name} =====", flush=True)
        txt, made = run_case_simple(name, down, enter, shots)
        picks = [ln.strip() for ln in txt.splitlines() if "boot-select: entry=" in ln]
        degrad = [
            ln.strip() for ln in txt.splitlines()
            if "no UEFI Runtime Services" in ln or "OsIndications" in ln
            or "BIOS boot" in ln or "no Windows boot option" in ln
        ]
        print(f"[keywalk:{name}] picks={picks}", flush=True)
        for d in degrad[:4]:
            print(f"[keywalk:{name}] degrade: {d}", flush=True)
        summary.append((name, picks, degrad, made))
        # 端口冷却，避免上一轮残留占用
        time.sleep(3.0)
    print("\n===== 汇总 =====")
    for name, picks, degrad, made in summary:
        print(f"{name:16s} picks={picks}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
