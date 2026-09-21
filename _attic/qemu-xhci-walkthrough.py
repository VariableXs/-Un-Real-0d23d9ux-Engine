#!/usr/bin/env python3
"""S4.1 · xHCI HID QEMU 走查（AI-5，2026-09-21）——USB 键鼠全链实机证据。

前置（脚本外）：
  1. kernel/ 下 `cargo kbuild`（新内核 ELF）；
  2. `python scripts/make-iso-qemu.py`（刷 isoroot → varix-qemu.iso）；
  3. 三证自查：源码 mtime < 构建 mtime < 本脚本运行时刻。

验证链（QEMU q35 + nec-usb-xhci + usb-kbd + usb-mouse，纯 HMP 注入）：
  A. 引导无回归：boot completed 出现（xHCI 探针在位不影响既有引导链）；
  B. 控制器证据：`xhci: controller at` → `xhci: init ok resets=0 devices=2`
     → 两台设备 kind=Keyboard / kind=Mouse 枚举完成 → channel live；
  C. USB 键盘事件：HMP sendkey 'a' → `xhci-hid: key A make=0x1e`（make 码
     与 PS/2 第一套表一致——同一汇点同构语义）；
  D. USB 鼠标事件 + Y 翻转契约：HMP mouse_move 向下（dy>0）→ 日志
     `mouse dx=0 dy=-N`（PS/2 语义负=向下，方向契约实机证据）+ 按键位图；
  E. 稳态无噪音：静置窗口内零未知事件（unk_ev 不增长）。

判定戒律（沿用 _attic/qemu-shell-walkthrough.py）：
  - 串口走 -serial tcp: socket 实时流（file: 落盘滞后可达分钟级）；
  - socket 连接后 settimeout(None)（超时传染戒律）；
  - TCG 键延迟 60~120s 波动 → STEP_TIMEOUT≥180s；
  - +1 标记基线必须先于发键取。

用法：python _attic/qemu-xhci-walkthrough.py
证据：_attic/qemu-xhci-serial.log（tee）+ _attic/acceptance-xhci/*.png

热复位缺口说明（2026-09-21 实测登记）：seed 配置 handoff=true 时内核会在
boot 完成后触发 BootNext+复位，QEMU 内第二遍 boot 的 xHCI 重初始化存在
未解缺口（第一遍全链成功、第二遍命令 TRB 对控制器不可见，trace 实证）。
本走查用 `handoff=0`（limine.conf cmdline 通道）锁定单轮 boot——热复位
重初始化作为登记缺口单列，不在本批验收路径内。
"""
import os
import re
import socket
import subprocess
import sys
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ATTIC = os.path.join(ROOT, "_attic")
ISO = os.path.join(ATTIC, "varix-xhci.iso")
MAKE_ISO = os.path.join(ROOT, "scripts", "make-iso-qemu.py")
CONF_SRC = os.path.join(ROOT, "build", "isoroot", "limine.conf")
CONF_DST = os.path.join(ATTIC, "limine-xhci.conf")
SERIAL = os.path.join(ATTIC, "qemu-xhci-serial.log")
SHOT_DIR = os.path.join(ATTIC, "acceptance-xhci")
MON_PORT = 14754
SERIAL_PORT = 14755
BOOT_TIMEOUT = 300
STEP_TIMEOUT = 180


class SerialTee:
    """串口 socket 实时流 → 内存缓冲 + 证据文件落盘。"""

    def __init__(self, port, path):
        for _ in range(50):
            try:
                self.sock = socket.create_connection(("127.0.0.1", port), timeout=0.3)
                break
            except OSError:
                time.sleep(0.2)
        else:
            raise RuntimeError("serial port never opened")
        self.sock.settimeout(None)  # 超时传染戒律
        self.buf = ""
        self.lock = threading.Lock()
        self.stop = False
        self.f = open(path, "w", encoding="utf-8", errors="replace")
        self.t = threading.Thread(target=self._pump, daemon=True)
        self.t.start()

    def _pump(self):
        while not self.stop:
            try:
                data = self.sock.recv(4096)
            except OSError:
                continue
            if not data:
                continue
            text = data.decode("utf-8", errors="replace")
            with self.lock:
                self.buf += text
                self.f.write(text)
                self.f.flush()

    def read(self):
        with self.lock:
            out = self.buf
            self.buf = ""
        return out

    def close(self):
        self.stop = True
        try:
            self.sock.close()
        except OSError:
            pass
        self.f.close()


class Mon:
    def __init__(self, port):
        for _ in range(50):
            try:
                self.sock = socket.create_connection(("127.0.0.1", port), timeout=0.3)
                break
            except OSError:
                time.sleep(0.2)
        else:
            raise RuntimeError("monitor port never opened")
        self.sock.settimeout(None)
        self.n = 0

    def cmd(self, line):
        self.sock.sendall((line + "\n").encode())
        time.sleep(0.15)

    def key(self, k):
        self.n += 1
        self.cmd(f"sendkey {k}")

    def mouse_move(self, dx, dy):
        self.n += 1
        self.cmd(f"mouse_move {dx} {dy}")

    def mouse_button(self, state):
        self.n += 1
        self.cmd(f"mouse_button {state}")

    def shot(self, name):
        os.makedirs(SHOT_DIR, exist_ok=True)
        self.cmd(f"screendump {os.path.join(SHOT_DIR, name)}")
        time.sleep(0.4)


def build_iso():
    """构建走查专用 ISO：cmdline 追加 handoff=0（单轮 boot 锁定）。"""
    with open(CONF_SRC, "r", encoding="utf-8") as f:
        conf = f.read()
    if "handoff=" not in conf:
        conf = conf.replace("kernel_cmdline: boot_timeout=0",
                            "kernel_cmdline: boot_timeout=0 handoff=0")
    with open(CONF_DST, "w", encoding="utf-8") as f:
        f.write(conf)
    r = subprocess.run([sys.executable, MAKE_ISO, "--conf", CONF_DST, "--out", ISO],
                       cwd=ROOT, capture_output=True, text=True)
    print(r.stdout.strip()[-200:] if r.stdout else "")
    if r.returncode != 0:
        print(r.stderr)
        return False
    return True


def main():
    if not os.path.exists(CONF_SRC):
        print(f"FATAL: {CONF_SRC} 不存在——先 cargo kbuild，再跑本脚本（它会自己打包 ISO）")
        return 2
    if not build_iso():
        return 2
    if os.path.exists(SERIAL):
        os.remove(SERIAL)
    proc = subprocess.Popen(
        [
            "qemu-system-x86_64",
            "-machine", "q35",
            "-cdrom", ISO,
            # S4.1 主角：xHCI 控制器 + USB 键鼠（显式钉到 xhci.0 总线，
            # 防 q35 默认 ich9-usb 抢挂）。
            "-device", "nec-usb-xhci,id=xhci",
            "-device", "usb-kbd,bus=xhci.0",
            "-device", "usb-mouse,bus=xhci.0",
            "-serial", f"tcp:127.0.0.1:{SERIAL_PORT},server,nowait",
            "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait",
            "-m", "1024",
        ],
        cwd=ROOT,
    )
    ser = SerialTee(SERIAL_PORT, SERIAL)
    checks = []
    try:
        mon = Mon(MON_PORT)

        def count_marker(pat):
            return len(re.findall(pat, ser.read()))

        def wait_count(pat, base, timeout, label):
            deadline = time.time() + timeout
            seen = 0
            while time.time() < deadline:
                seen = count_marker(pat)
                if seen > base:
                    print(f"  [PASS] {label} (count={seen})")
                    return True
                time.sleep(1.0)
            print(f"  [FAIL] {label} (count={seen}, base={base})")
            return False

        # A. 控制器证据（探针在引导链早段，先于 ushell）。
        checks.append(("B1 xhci controller found (pci scan)",
                       wait_count(r"xhci: controller at", 0, BOOT_TIMEOUT, "B1 controller found")))
        checks.append(("B2 xhci init ok (resets=0 devices=2)",
                       wait_count(r"xhci: init ok resets=\d+ devices=2", 0, 60, "B2 init ok devices=2")))
        checks.append(("B3 keyboard enumerated",
                       wait_count(r"xhci: port \d+ speed=\d+ enumerated kind=Keyboard", 0, 60, "B3 kbd enum")))
        checks.append(("B4 mouse enumerated",
                       wait_count(r"xhci: port \d+ speed=\d+ enumerated kind=Mouse", 0, 60, "B4 mouse enum")))
        checks.append(("B5 channel live",
                       wait_count(r"xhci: usb keyboard/mouse channel live", 0, 60, "B5 channel live")))
        mon.shot("01-boot.png")

        # C. USB 键盘事件：按 'a' → HID usage 0x04 → Key::A → make 0x1e。
        base_key = count_marker(r"xhci-hid: key ")
        mon.key("a")
        checks.append(("C1 usb key A flows to funnel (make=0x1e)",
                       wait_count(r"xhci-hid: key A make=0x1e", base_key, STEP_TIMEOUT, "C1 key A")))
        time.sleep(0.8)
        mon.shot("02-key-a.png")

        # D. USB 鼠标事件 + Y 翻转契约：向下移动（HMP dy>0）→ 日志 dy<0。
        base_mouse = count_marker(r"xhci-hid: mouse ")
        mon.mouse_move(0, 30)
        checks.append(("D1 usb mouse move reaches funnel",
                       wait_count(r"xhci-hid: mouse dx=-?\d+ dy=-?\d+", base_mouse, STEP_TIMEOUT, "D1 mouse move")))
        time.sleep(1.0)
        # 方向契约：HMP 向下 30 → 最后一条 mouse 日志 dy 为负。
        with open(SERIAL, "r", encoding="utf-8", errors="replace") as f:
            log = f.read()
        ms = re.findall(r"xhci-hid: mouse dx=(-?\d+) dy=(-?\d+)", log)
        if ms:
            dx, dy = map(int, ms[-1])
            ok = dy < 0
            checks.append((f"D2 y-axis inverted (hmp dy=+30 -> log dy={dy})", ok))
        else:
            checks.append(("D2 y-axis inverted (no mouse log)", False))
        # 按键：左键按下 → buttons bit0。
        base_mouse = count_marker(r"xhci-hid: mouse ")
        mon.mouse_button("l")
        time.sleep(0.5)
        mon.mouse_button("r")  # 释放（QEMU mouse_button 参数=按下的键集合）
        ok = wait_count(r"xhci-hid: mouse .*buttons=0x0[1-7]", base_mouse, STEP_TIMEOUT, "D3 mouse click")
        checks.append(("D3 usb mouse button event", ok))
        mon.shot("03-mouse.png")

        # E. 稳态无噪音：静置 5s 后未知事件不增长。
        with open(SERIAL, "r", encoding="utf-8", errors="replace") as f:
            unk_before = len(re.findall(r"xhci: init ok .*unk_ev=(\d+)", f.read()))
        time.sleep(5)
        with open(SERIAL, "r", encoding="utf-8", errors="replace") as f:
            log = f.read()
        m = re.findall(r"unk_ev=(\d+)", log)
        unk_after = int(m[-1]) if m else -1
        checks.append((f"E1 no unknown events (unk_ev={unk_after})", unk_after == 0))
        mon.shot("04-idle.png")

        # F. 引导回归：boot completed 判定走**证据文件**（增量缓冲只含
        #    等待窗口内的新行，历史行会被漏判——首轮走查 F1 误报的根因）。
        with open(SERIAL, "r", encoding="utf-8", errors="replace") as f:
            full = f.read()
        checks.append(("F1 boot completed (no boot regression)",
                       bool(re.search(r"boot completed", full))))
    finally:
        time.sleep(1.0)
        ser.close()
        try:
            proc.terminate()
            proc.wait(timeout=15)
        except Exception:
            proc.kill()

    print("\n==== S4.1 xHCI QEMU 走查结果 ====")
    failed = 0
    for name, ok in checks:
        print(f"  {'PASS' if ok else 'FAIL'}  {name}")
        if not ok:
            failed += 1
    print(f"==== {len(checks) - failed}/{len(checks)} PASS ====")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
