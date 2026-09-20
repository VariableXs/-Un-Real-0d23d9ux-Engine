#!/usr/bin/env python3
"""三卡菜单：按键 → 视觉高亮的严格时序实验。

判据必须**同时**满足：
  * 视觉：HMP screendump 里高亮框移到目标卡（按 y 分块定位）
  * 逻辑：串口出现 `boot-select: entry=<目标>`

为什么单独做这个：常规走查脚本在 t=16s 发键、t=24s 截图，「发键」与「截图」
之间没有对齐到内核的绘制时刻；且 `down 后 0.8s 再 Enter` 会让菜单来不及重绘
就被 Enter 结束（Enter 一返回内核立刻切走，menu 帧被覆盖）。

本脚本：发键后**逐秒连续截图**，直到串口出现 entry= 或超时，
从而拿到「按下之后的每一帧」，不依赖任何猜测的时刻。

用法：python menu-key-visual.py [down次数] [是否Enter] [总秒数]
证据：_attic/acceptance-menu/key-visual/
"""
import os
import socket
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ATTIC = os.path.join(ROOT, "_attic")
QEMU = r"C:\Program Files\qemu\qemu-system-x86_64"
CODE = os.path.join(ATTIC, "edk2-x86_64-code.fd")
VARS = os.path.join(ATTIC, "edk2-vars-keyvis.fd")
IMG = os.path.join(ATTIC, "varix-uefi.img")
OUT = os.path.join(ATTIC, "acceptance-menu", "key-visual")
SER = os.path.join(ATTIC, "keyvis-serial.log")
MON_PORT = 14961
SER_PORT = 14962
NL = bytes([10])


class Bus:
    """极简串口泵（先 Popen 再 connect，超时不传染）。"""

    def __init__(self, port, path):
        self.buf = bytearray()
        self.sock = None
        self.port = port
        self.f = open(path, "wb")

    def connect(self):
        for _ in range(400):
            try:
                self.sock = socket.create_connection(("127.0.0.1", self.port), timeout=1)
                break
            except OSError:
                time.sleep(0.1)
        if self.sock:
            self.sock.settimeout(0.4)

    def pump(self):
        if not self.sock:
            return
        try:
            d = self.sock.recv(65536)
            if d:
                self.buf.extend(d)
                self.f.write(d)
                self.f.flush()
        except OSError:
            pass

    def text(self):
        return bytes(self.buf).decode("utf-8", "replace")


def main():
    down = int(sys.argv[1]) if len(sys.argv) > 1 else 1
    enter = (sys.argv[2].lower() in ("1", "true", "yes")) if len(sys.argv) > 2 else True
    total = int(sys.argv[3]) if len(sys.argv) > 3 else 70
    os.makedirs(OUT, exist_ok=True)
    if not os.path.exists(VARS):
        with open(VARS, "wb") as f:
            f.write(b"\x00" * (256 * 1024))

    cmd = [
        QEMU, "-machine", "q35", "-m", "2048", "-smp", "2", "-cpu", "max",
        "-drive", "if=pflash,format=raw,file=%s,unit=0,readonly=on" % CODE,
        "-drive", "if=pflash,format=raw,file=%s,unit=1" % VARS,
        "-drive", "file=%s,format=raw,if=ide,index=0" % IMG,
        "-boot", "order=c",
        "-serial", "tcp:127.0.0.1:%d,server,nowait" % SER_PORT,
        "-monitor", "tcp:127.0.0.1:%d,server,nowait" % MON_PORT,
        "-display", "none",
    ]
    if os.path.exists(SER):
        os.remove(SER)
    p = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    bus = Bus(SER_PORT, SER)
    bus.connect()

    mon = None
    for _ in range(200):
        try:
            mon = socket.create_connection(("127.0.0.1", MON_PORT), timeout=1)
            break
        except OSError:
            time.sleep(0.1)
    mon.settimeout(None)

    def shot(name):
        mon.sendall(b"screendump " + os.path.join(OUT, name).replace("\\", "/").encode() + NL)
        time.sleep(0.5)

    def key(k):
        mon.sendall(b"sendkey %s" % k.encode() + NL)
        time.sleep(0.08)

    t0 = time.time()
    keys_done = False
    got = None
    shots = []
    while time.time() - t0 < total:
        elapsed = time.time() - t0
        bus.pump()

        # 内核就绪即发键（等串口出现 boot-diag 行，说明菜单已在绘制）
        if not keys_done and "boot-diag:" in bus.text() and elapsed > 3:
            time.sleep(1.0)                      # 让菜单画完第一帧
            for _ in range(down):
                key("down")
                time.sleep(1.5)                  # 给内核足够时间重绘
            shot("press-down.png")               # ← Enter 之前的画面
            print("[keyvis] t=%.1fs 已发 down x%d 并截图" % (time.time() - t0, down), flush=True)
            if enter:
                key("ret")
            keys_done = True

        # 发键后逐秒连拍，抓到每一帧
        if keys_done and len(shots) < 12:
            tag = "post-%02d" % len(shots)
            shot(tag + ".png")
            shots.append(tag)
            time.sleep(1.0)

        if "boot-select: entry=" in bus.text():
            got = [ln.strip() for ln in bus.text().splitlines() if "boot-select: entry=" in ln]
            break
    else:
        pass

    bus.pump()
    try:
        mon.close()
    except Exception:
        pass
    p.terminate()
    try:
        p.wait(timeout=5)
    except subprocess.TimeoutExpired:
        p.kill()
    bus.f.close()

    txt = open(SER, "rb").read().decode("utf-8", "replace")
    picks = [ln.strip() for ln in txt.splitlines() if "boot-select: entry=" in ln]
    print("\n=== 结果 ===")
    print("down=%d enter=%s" % (down, enter))
    print("串口 picks: %s" % (picks or "（未出现 entry= 行）"))
    print("截图目录: %s" % OUT)
    # 逐张报告高亮块位置
    from PIL import Image
    for f in sorted(os.listdir(OUT)):
        if not f.endswith(".png"):
            continue
        im = Image.open(os.path.join(OUT, f)).convert("RGB")
        px = im.load()
        ys = [y for y in range(im.size[1])
              if (px[400, y][2] > 180 and px[400, y][0] < 130 and px[400, y][1] > 100)]
        groups = []
        for y in ys:
            if groups and y - groups[-1][-1] <= 3:
                groups[-1].append(y)
            else:
                groups.append([y])
        spans = [(g[0], g[-1]) for g in groups if g[-1] - g[0] >= 8]
        print("  %-20s 高亮块=%s" % (f, spans))


if __name__ == "__main__":
    main()
