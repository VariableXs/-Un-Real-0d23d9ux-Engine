#!/usr/bin/env python3
"""三卡菜单侦察运行：纯 UEFI 引导带菜单 ISO，只收串口，不做键注入。

目的：摸清「菜单亮出前 / 亮出期间 / 选中后」串口行的真实时序，为正式走查
脚本挑选可靠判据标记（menumark / navmark / selmark）。

用法：python _attic/tools/menu-recon.py [seconds]
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
QEMU = r"C:\Program Files\qemu\qemu-system-x86_64"
EDK2 = os.path.join(ATTIC, "edk2-x86_64-code.fd")
LOG = os.path.join(ATTIC, "menu-recon-serial.log")
MON_PORT = 14801
SER_PORT = 14802

RUN_SECS = int(sys.argv[1]) if len(sys.argv) > 1 else 150
# mode: uefi（OVMF pflash，能测 BootNext）/ bios（SeaBIOS，Limine-on-ISO 可用）
# 实测：Limine 在 ISO9660/El Torito + OVMF 下报 "Could not meaningfully match the
# boot device handle" 并停在 "Press any key" —— 故菜单/键盘走查用 bios 模式。
MODE = sys.argv[2].lower() if len(sys.argv) > 2 else "bios"


class Pump(threading.Thread):
    def __init__(self, port, path):
        super().__init__(daemon=True)
        self.buf = b""
        self.f = open(path, "wb")
        self.alive = True
        self.sock = None
        self.port = port

    def connect(self):
        """必须在 QEMU 启动**之后**调用：server,nowait 端口只有那时才在听。"""
        for _ in range(200):
            try:
                self.sock = socket.create_connection(("127.0.0.1", self.port), timeout=5)
                break
            except OSError:
                time.sleep(0.1)
        if self.sock is not None:
            # 超时传染戒律：create_connection 的 timeout 会被 recv 继承
            self.sock.settimeout(None)

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


NL = bytes([10])  # HMP 命令终止符（避免源码里出现裸换行字面量）

def _shot(mon_port, name):
    d = os.path.join(ATTIC, "acceptance-menu")
    os.makedirs(d, exist_ok=True)
    try:
        s = socket.create_connection(("127.0.0.1", mon_port), timeout=5)
        try:
            s.recv(65536)
        except Exception:
            pass
        s.sendall(b"screendump " + os.path.join(d, name).encode() + NL)
        time.sleep(1.2)
        try:
            s.recv(65536)
        except Exception:
            pass
        s.close()
    except OSError:
        pass


def main() -> int:
    if not os.path.isfile(ISO):
        print("ERROR: 缺 " + ISO, file=sys.stderr)
        return 1
    for p in (LOG,):
        if os.path.exists(p):
            os.remove(p)
    pump = Pump(SER_PORT, LOG)

    proc = subprocess.Popen(
        [
            QEMU,
            "-machine", "q35", "-m", "2G", "-smp", "2", "-cpu", "max",
            *([ "-drive", f"if=pflash,format=raw,file={EDK2},unit=0" ] if MODE == "uefi" else []),
            "-cdrom", ISO,
            "-boot", "order=d",
            "-serial", f"tcp:127.0.0.1:{SER_PORT},server,nowait",
            "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait",
            "-display", "none",
        ],
        stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT,
    )
    print(f"[recon] QEMU pid={proc.pid}  ISO={os.path.basename(ISO)}")
    pump.connect()
    if pump.sock is None:
        print("[recon] ERROR: 串口连不上", file=sys.stderr)
        proc.kill()
        return 1
    pump.start()
    t0 = time.time()
    marked = None
    shots = [2, 6, 12, 25, 45]
    done_shots = set()
    try:
        while time.time() - t0 < RUN_SECS:
            time.sleep(0.5)
            el = time.time() - t0
            if proc.poll() is not None:
                print(f"[recon] QEMU exited rc={proc.returncode}")
                break
            txt = pump.text()
            if marked is None and "boot-select: entry=" in txt:
                marked = el
                for ln in txt.splitlines():
                    if "boot-select: entry=" in ln:
                        print(f"[recon] ** MENU EXIT at t={el:.1f}s -> {ln.strip()}", flush=True)
                        break
            if int(el) % 5 == 0:
                n = len(txt.splitlines())
                print(f"[recon] t={int(el):3d}s serial_lines={n}", flush=True)
            for s_at in shots:
                if s_at not in done_shots and el >= s_at:
                    done_shots.add(s_at)
                    _shot(MON_PORT, f"recon-t{s_at:02d}s.png")
                    print(f"[recon] shot t={s_at}s", flush=True)
    except KeyboardInterrupt:
        pass
    if marked is None:
        print("[recon] ** 菜单在观测窗口内未退出（仍在等待按键）", flush=True)
    # 收尾截图（菜单应在屏上）
    try:
        s = socket.create_connection(("127.0.0.1", MON_PORT), timeout=5)
        s.recv(65536)
        s.sendall(b"screendump " + os.path.join(ATTIC, "acceptance-menu", "recon-menu.png").encode() + b"\n")
        time.sleep(1.5)
        try:
            s.recv(65536)
        except Exception:
            pass
        s.sendall(b"quit\n")
        s.close()
    except OSError as e:
        print("[recon] monitor 不可用:", e)
    pump.alive = False
    try:
        proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        proc.kill()

    txt = pump.text()
    print("---- serial (%d lines) ----" % len(txt.splitlines()))
    for i, ln in enumerate(txt.splitlines(), 1):
        print(f"{i:4d}| {ln}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
