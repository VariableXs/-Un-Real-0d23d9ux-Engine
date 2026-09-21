#!/usr/bin/env python3
"""AI-1 部署预检：对已刷盘内核对应的 varix-qemu.iso（HEAD b0beac22 批次）做引导冒烟。

判定（缓冲存在性，独立 QEMU 独立缓冲）：
  - timeout=5 default=varix   （种子配置经 internal module 生效）
  - refusing to guess          （防自锁闸门在 HEAD 内核仍工作）
  - boot completed             （引导不炸、落 ushell）
  - xhci 相关行仅收集为信息（QEMU 默认机无 xHCI 设备 → 预期优雅跳过）
产物：_attic/ai1-deploy-smoke-serial.log
"""

import os
import socket
import subprocess
import sys
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ATTIC = os.path.join(ROOT, "_attic")
ISO = os.path.join(ROOT, "varix-qemu.iso")
LOG = os.path.join(ATTIC, "ai1-deploy-smoke-serial.log")
SERIAL_PORT = 14778
MON_PORT = 14779
TIMEOUT = 420


class SerialTee:
    def __init__(self, port, path):
        self.lock = threading.Lock()
        self.buf = b""
        self.alive = True
        self.f = open(path, "wb")
        self.sock = self._connect(port)
        self.sock.settimeout(None)
        threading.Thread(target=self._pump, daemon=True).start()

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
                data = self.sock.recv(65536)
            except OSError:
                break
            if not data:
                break
            with self.lock:
                self.buf += data
            self.f.write(data)
            self.f.flush()

    def count(self, marker):
        with self.lock:
            return self.buf.count(marker.encode())

    def text(self):
        with self.lock:
            return self.buf.decode(errors="replace")

    def close(self):
        self.alive = False
        try:
            self.sock.close()
        except Exception:
            pass
        self.f.close()


def main() -> int:
    if not os.path.isfile(ISO):
        raise SystemExit("missing ISO: " + ISO)
    if os.path.exists(LOG):
        os.remove(LOG)
    proc = subprocess.Popen(
        ["qemu-system-x86_64", "-m", "1024", "-cdrom", ISO, "-display", "none",
         "-no-reboot",
         "-serial", f"tcp:127.0.0.1:{SERIAL_PORT},server,nowait",
         "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait"],
        cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    ok = True
    try:
        serial = SerialTee(SERIAL_PORT, LOG)
        mon = socket.create_connection(("127.0.0.1", MON_PORT), timeout=10)
        time.sleep(0.5)
        mon.recv(65536)

        def hmp(cmd):
            try:
                mon.sendall(cmd.encode() + b"\n")
            except OSError:
                pass
            time.sleep(1.0)

        t0 = time.time()
        # 阶段 1：等菜单引导诊断（配置桥生效证据）
        while time.time() - t0 < TIMEOUT and serial.count("timeout=5 default=varix") == 0:
            time.sleep(1)
        # 阶段 2：等输入探针进入 HMP 等待窗口，然后喂齐退出矩阵
        # （Up/Down/Enter + ≥2 鼠标移动 + 左键按下——input_probe 的四件套）
        t1 = time.time()
        while time.time() - t1 < TIMEOUT and serial.count("awaiting sendkey") == 0:
            time.sleep(1)
        if serial.count("awaiting sendkey") > 0:
            print("  [feed] HMP key/mouse -> input-probe exit matrix")
            for k in ("up", "down", "ret"):
                hmp("sendkey " + k)
                hmp("sendkey " + k)  # 按下+释放，避免键状态卡住
            for _ in range(3):
                hmp("mouse_move 0 12 9")
                hmp("mouse_move 0 7 5")
            hmp("mouse_button 1")
            hmp("mouse_button 0")
        # 阶段 3：等引导终点（TCG 宽窗）
        t2 = time.time()
        while time.time() - t2 < TIMEOUT and serial.count("boot completed") == 0:
            time.sleep(1)
        time.sleep(2)
        try:
            mon.sendall(b"quit\n")
        except OSError:
            pass
        checks = [
            "timeout=5 default=varix",
            "refusing to guess",
            "boot completed",
        ]
        for m in checks:
            hit = serial.count(m) > 0
            print(f"  {m}: {'PASS' if hit else 'FAIL'}")
            ok &= hit
        text = serial.text()
        xhci_lines = [ln for ln in text.splitlines() if "xhci" in ln.lower()]
        print(f"  xhci 行 {len(xhci_lines)} 条（信息项）:")
        for ln in xhci_lines[:8]:
            print("   ", ln.strip()[:150])
        boot_ms = [ln for ln in text.splitlines() if "boot completed" in ln]
        if boot_ms:
            print("  ", boot_ms[-1].strip()[:120])
        try:
            proc.wait(timeout=30)
        except subprocess.TimeoutExpired:
            proc.kill()
        serial.close()
    finally:
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()
    print("DEPLOY SMOKE VERDICT:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
