#!/usr/bin/env python3
r"""三卡菜单**鼠标**走查（OVMF + GPT/ESP 磁盘镜像）——需求 1/6「任意鼠标和键盘」的实证据。

背景（为什么要单独做这一轮）：此前所有菜单走查（u1/u2/u3）都是 sendkey
键盘注入，**鼠标从未被验证过**。核查发现内核里鼠标驱动虽有（`inputsvc`
的 AUX 通道 + MouseDecoder），但 `mouse_bringup` 只挂在 QEMU 探针分支、
且探针排在菜单之后——真机鼠标根本没初始化，菜单也只吃键盘。本轮补上的
接线必须用真机式证据钉死。

判据（串口）：
  * 前置：`boot-diag: mouse bringup set-defaults=true enable-report=true`
    —— 证明鼠标在菜单亮出**之前**已完成 i8042 AUX 使能（0xF6/0xF4 双 ACK）；
  * m1 悬停+点击中间卡 → `boot-select: entry=windows`
    （指针初始在屏幕正中，几何上必落在中间那张卡上）；
  * m2 下移到第三张卡+点击 → `boot-select: entry=uefi`；
  * m3 只移动不点击 → `boot-select: entry=varix`
    （悬停只是预选，**不确认**，倒计时归零走默认项——与键盘语义一致）。

用法：python _attic/tools/menu-mouse-walk.py [场景名...]
证据：_attic/acceptance-menu/mouse/<场景>-*.png + mouse-<场景>.log
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
SHOT_DIR = os.path.join(ATTIC, "acceptance-menu", "mouse")
QEMU = r"C:\Program Files\qemu\qemu-system-x86_64"

MON_PORT = 14851
SER_PORT = 14852
NL = bytes([10])


class Pump(threading.Thread):
    """串口采集器：循环抽空、对端关闭即重连补收，绝不静默退出（丢包戒律）。"""

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

    def move(self, dx, dy, dz=0):
        self.cmd(f"mouse_move {dx} {dy} {dz}")

    def button(self, state):
        """state: 位掩码 1=左键 2=右键 4=中键；0 = 全部松开。"""
        self.cmd(f"mouse_button {state}")

    def click(self):
        self.button(1)
        time.sleep(0.6)
        self.button(0)

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
    if not os.path.isfile(EDK2_VARS):
        with open(EDK2_VARS, "wb") as f:
            f.write(b"\x00" * (256 * 1024))
        print(f"新建 OVMF NVRAM: {EDK2_VARS}")


def run_case(name, script, shots, total=180):
    """script: [(时刻秒, 动作)]，动作为可调用 mon 的 lambda。"""
    ensure_vars()
    ser_log = os.path.join(ATTIC, "acceptance-menu", f"mouse-{name}.log")
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
    # 顺序铁律：先 Popen（端口才开始听）→ 再 connect → 再 start 线程
    pump.connect()
    pump.start()
    print(f"[{name}] QEMU pid={proc.pid}", flush=True)

    t0 = time.time()
    mon, done = None, []
    try:
        while time.time() - t0 < total:
            time.sleep(0.25)
            el = time.time() - t0
            if proc.poll() is not None:
                print(f"[{name}] QEMU exited rc={proc.returncode}", flush=True)
                break
            if mon is None:
                try:
                    mon = Mon(MON_PORT)
                except OSError:
                    continue
            for at, tag, act in shots:
                if tag not in done and el >= at:
                    mon.shot(f"{name}-{tag}.png")
                    done.append(tag)
                    print(f"[{name}] shot t={el:.1f}s {tag}", flush=True)
            for at, tag, act in script:
                if tag not in done and el >= at:
                    act(mon)
                    done.append(tag)
                    print(f"[{name}] act t={el:.1f}s {tag}", flush=True)
            if "boot-select: entry=" in pump.text() and el > 40:
                break
    finally:
        if mon is not None:
            mon.shot(f"{name}-final.png")
            mon.close()
        pump.alive = False
        try:
            proc.wait(timeout=8)
        except subprocess.TimeoutExpired:
            proc.kill()
    return pump.text(), done


# 指针初始在屏幕正中（几何上必落在中间那张卡 = WINDOWS 上）。
# m1：极小位移让指针显形并悬停 → 左键确认 → windows
# m2：下移 90px 到第三张卡（UEFI）→ 左键确认 → uefi
# m3：同样下移到第三张卡，但**不点击** → 倒计时归零走默认项 varix
CASES = {
    "m1-hover-click-middle": (
        [(22, "hover", lambda m: m.move(1, 0)),
         (26, "click", lambda m: m.click())],
        [(18, "menu", None), (24, "after-hover", None), (30, "after-click", None)],
    ),
    "m2-move-third-click": (
        [(22, "move", lambda m: m.move(0, 90)),
         (26, "click", lambda m: m.click())],
        [(18, "menu", None), (24, "after-move", None), (30, "after-click", None)],
    ),
    "m3-move-third-no-click": (
        [(22, "move", lambda m: m.move(0, 90))],
        [(18, "menu", None), (24, "after-move", None), (70, "settled", None)],
    ),
}


def main() -> int:
    which = sys.argv[1:] or list(CASES.keys())
    ok = True
    for name in which:
        if name not in CASES:
            print("未知场景 " + name, file=sys.stderr)
            return 1
        script, shots = CASES[name]
        print(f"\n===== 鼠标场景 {name} =====", flush=True)
        txt, _ = run_case(name, script, shots)
        for key in ("mouse bringup", "boot-select: entry=", "BootNext", "OsIndications",
                    "no Windows boot option"):
            for ln in txt.splitlines():
                if key in ln:
                    print("   | " + ln.strip())
        picks = [ln.strip() for ln in txt.splitlines() if "boot-select: entry=" in ln]
        got = picks[-1].split("entry=")[-1].strip() if picks else "(未捕获)"
        want = {"m1-hover-click-middle": "windows",
                "m2-move-third-click": "uefi",
                "m3-move-third-no-click": "varix"}[name]
        good = got == want
        ok = ok and good
        print(f"   => {name}: entry={got} 期望={want} {'PASS' if good else 'FAIL'}")
    print("\n总判定:", "ALL-PASS" if ok else "HAS-FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
