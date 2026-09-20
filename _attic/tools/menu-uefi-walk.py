#!/usr/bin/env python3
r"""UEFI 三卡菜单走查（OVMF + GPT/ESP 磁盘镜像）——真验证 BootNext / OsIndications。

为什么必须用磁盘镜像而不是 ISO：
  Limine 在 ISO9660/El Torito + OVMF 下报
  "Could not meaningfully match the boot device handle" 并停在 "Press any key"，
  内核根本不启动（实测确认）。GPT+ESP 是普通块设备，不受此限。

判据（UEFI 下 Runtime Services 可用，这才是真通道）：
  * varix  → `boot-select: entry=varix` + 正常起内核
  * windows→ `boot-select: Windows boot option resolved to 0x%04X`（或 unverified）
             + `BootNext=0x%04X written & verified — resetting`
  * uefi   → `boot-select: requesting firmware setup via OsIndications`
             + `OsIndications` 写成功（或 unsupported 降级）

用法：python _attic/tools/menu-uefi-walk.py [场景名...]
证据：_attic/acceptance-menu/uefi/<场景>-*.png + uefi-<场景>.log
"""
import os
import socket
import subprocess
import sys
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ATTIC = os.path.join(ROOT, "_attic")
IMG = os.path.join(ATTIC, "varix-uefi.img")
EDK2 = os.path.join(ATTIC, "edk2-x86_64-code.fd")
EDK2_VARS = os.path.join(ATTIC, "edk2-vars-menu.fd")
SHOT_DIR = os.path.join(ATTIC, "acceptance-menu", "uefi")
QEMU = r"C:\Program Files\qemu\qemu-system-x86_64"

MON_PORT = 14841
SER_PORT = 14842
NL = bytes([10])


class Pump(threading.Thread):
    def __init__(self, port, path):
        super().__init__(daemon=True)
        self.buf = b""
        self.f = open(path, "wb")
        self.alive = True
        self.sock = None
        self.port = port

    def connect(self):
        for _ in range(400):
            try:
                self.sock = socket.create_connection(("127.0.0.1", self.port), timeout=5)
                break
            except OSError:
                time.sleep(0.1)
        if self.sock is not None:
            self.sock.settimeout(None)  # 超时传染戒律

    def run(self):
        """采集循环（**必须先 Popen 再 connect**，见 connect 调用点）。

        【踩过的坑 · 采集器丢包】早先版本这里是「recv 一次 → 空就 break」，
        于是行尾会被切掉：实测同一场景下简易泵只收到 1544 字节、
        严格泵收到 3656 字节，日志行 `boot-select: entry=varix` 被切成
        `entry=v`，一度被误判成「内核日志被截断/帧缓冲越界写敲到 COM1」。
        真相纯属采集侧丢包 —— **内核无罪**。采集器必须循环抽空、只在
        对端真正关闭（recv 返回空）时才重连补收，绝不静默退出。
        """
        while self.alive:
            if self.sock is None:
                self.connect()
                if self.sock is None:
                    time.sleep(0.1)
                    continue
            try:
                d = self.sock.recv(65536)
            except OSError:
                self.sock = None
                continue
            if not d:
                # 对端关闭：立刻重连补收，绝不静默放弃（丢掉的就是结论）。
                try:
                    self.sock.close()
                except Exception:
                    pass
                self.sock = None
                continue
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
            time.sleep(0.4)
            self.s.close()
        except Exception:
            pass


def ensure_vars():
    """OVMF 可变存储（NVRAM）：首次从零填充，之后保留（BootNext 就是写在这里）。"""
    if not os.path.isfile(EDK2_VARS):
        with open(EDK2_VARS, "wb") as f:
            f.write(b"\x00" * (256 * 1024))
        print(f"新建 OVMF NVRAM: {EDK2_VARS}")
    else:
        print(f"复用 OVMF NVRAM: {EDK2_VARS}")


def run_case(name, down_presses, press_enter, shots, key_delay=16.0, total=150):
    ensure_vars()
    ser_log = os.path.join(ATTIC, "acceptance-menu", f"uefi-{name}.log")
    os.makedirs(os.path.dirname(ser_log), exist_ok=True)
    if os.path.exists(ser_log):
        os.remove(ser_log)
    pump = Pump(SER_PORT, ser_log)
    proc = subprocess.Popen(
        [
            QEMU, "-machine", "q35", "-m", "2048", "-smp", "2", "-cpu", "max",
            "-drive", f"if=pflash,format=raw,file={EDK2},unit=0,readonly=on",
            "-drive", f"if=pflash,format=raw,file={EDK2_VARS},unit=1",
            "-drive", f"file={IMG},format=raw,if=ide,index=0",
            "-boot", "order=c",
            "-serial", f"tcp:127.0.0.1:{SER_PORT},server,nowait",
            "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait",
            "-display", "none",
        ],
        cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT,
    )
    # 顺序铁律：先 Popen（端口才开始听）→ 再 connect → 再 start 线程。
    # 反过来的写法会让 connect 的 400 次重试全部落空。
    pump.connect()
    pump.start()
    print(f"[uefi:{name}] QEMU pid={proc.pid}", flush=True)

    t0 = time.time()
    mon, keys_sent, made = None, False, []
    try:
        while time.time() - t0 < total:
            time.sleep(0.25)
            el = time.time() - t0
            if proc.poll() is not None:
                print(f"[uefi:{name}] QEMU exited rc={proc.returncode}", flush=True)
                break
            if mon is None:
                try:
                    mon = Mon(MON_PORT)
                except OSError:
                    continue
            for at, tag in shots:
                if tag not in made and el >= at:
                    mon.shot(f"{name}-{tag}.png")
                    made.append(tag)
                    print(f"[uefi:{name}] shot t={el:.1f}s {tag}", flush=True)
            if not keys_sent and el >= key_delay:
                keys_sent = True
                for _ in range(down_presses):
                    mon.key("down")
                    time.sleep(0.8)
                if press_enter:
                    mon.key("ret")
                print(f"[uefi:{name}] keys@t={el:.1f}s down x{down_presses} enter={press_enter}",
                      flush=True)
            if "boot-select: entry=" in pump.text() and el > key_delay + 10:
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
    "u1-baseline": (0, False, [(20, "menu"), (80, "after")]),
    "u2-down1-enter": (1, True, [(16, "menu"), (24, "after-down"), (34, "after-enter")]),
    "u3-down2-enter": (2, True, [(16, "menu"), (26, "after-down"), (36, "after-enter")]),
}


def main() -> int:
    which = sys.argv[1:] or list(CASES.keys())
    out = []
    for name in which:
        if name not in CASES:
            print("未知场景 " + name, file=sys.stderr)
            return 1
        down, enter, shots = CASES[name]
        print(f"\n===== UEFI 场景 {name} =====", flush=True)
        txt, made = run_case(name, down, enter, shots)
        picks = [ln.strip() for ln in txt.splitlines() if "boot-select: entry=" in ln]
        chan = [ln.strip() for ln in txt.splitlines()
                if any(k in ln for k in ("BootNext", "OsIndications", "identity-mapped",
                                          "resolved to", "no Windows boot option",
                                          "firmware reset", "guessing"))]
        print(f"[uefi:{name}] picks={picks}", flush=True)
        for c in chan[:8]:
            print(f"[uefi:{name}] chan: {c}", flush=True)
        out.append((name, picks, chan))
        time.sleep(3.0)
    print("\n===== 汇总 =====")
    for n, p, c in out:
        print(f"{n:16s} {p}")
        for x in c[:6]:
            print(f"    {x}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
