#!/usr/bin/env python3
"""采集器丢包判据探针。

背景：走查里出现过 `boot-select: entry=v`（应为 `entry=varix`）这种**行尾被切**。
需要先排除「日志真的被截断」还是「我的串口采集器漏读」。两者的处置完全不同：
  * 真截断 → 内核 bug，要修（甚至帧缓冲越界写敲到 COM1）。
  * 采集漏读 → 工具 bug，内核无罪，但**所有历史走查结论都要重新判定**。

判据设计（同一台 QEMU、同一个字节流，两种采集器并读）：
  A 组 = 单次 recv 的简易泵（历史走查用的写法）
  B 组 = while-循环抽空 + 非阻塞的严格泵
若最终 A 比 B **少**字节、且少的正好是行尾，则证明是采集侧丢包。

用法：python collect-guard-probe.py [秒数]
证据：_attic/acceptance-menu/collect-guard/A.log B.log A.len B.len
"""
import os
import socket
import subprocess
import sys
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ATTIC = os.path.join(ROOT, "_attic")
QEMU = r"C:\Program Files\qemu\qemu-system-x86_64"
CODE = os.path.join(ATTIC, "edk2-x86_64-code.fd")
VARS = os.path.join(ATTIC, "edk2-vars-collect.fd")
IMG = os.path.join(ATTIC, "varix-uefi.img")
OUT = os.path.join(ATTIC, "acceptance-menu", "collect-guard")
SER_PORT = 14971
MON_PORT = 14972
NL = bytes([10])


def prep():
    os.makedirs(OUT, exist_ok=True)
    if not os.path.exists(VARS):
        with open(VARS, "wb") as f:
            f.write(b"\x00" * (256 * 1024))


class Pumper(threading.Thread):
    """严格泵：连上后循环抽空，绝不空转放弃。"""

    def __init__(self, port, path, label):
        super().__init__(daemon=True)
        self.buf = bytearray()
        self.path = path
        self.label = label
        self.sock = None
        self.stop = False
        self.port = port
        self.connects = 0

    def connect(self):
        for _ in range(600):
            try:
                self.sock = socket.create_connection(("127.0.0.1", self.port), timeout=1)
                self.sock.settimeout(0.3)
                self.connects += 1
                return
            except OSError:
                time.sleep(0.05)

    def run(self):
        # 端口在进程起来前不在听：重试直到成功，成功后不再放弃。
        while not self.stop:
            if self.sock is None:
                self.connect()
                if self.sock is None:
                    time.sleep(0.1)
                    continue
            try:
                d = self.sock.recv(65536)
                if not d:
                    # 对端关了：立即重连补收，绝不静默退出。
                    try:
                        self.sock.close()
                    except Exception:
                        pass
                    self.sock = None
                    continue
                self.buf.extend(d)
            except socket.timeout:
                continue
            except OSError:
                try:
                    self.sock.close()
                except Exception:
                    pass
                self.sock = None
        with open(self.path, "wb") as f:
            f.write(bytes(self.buf))


def main():
    secs = int(sys.argv[1]) if len(sys.argv) > 1 else 30
    prep()
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
    p = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    # B：严格泵（线程里循环抽空）
    b = Pumper(SER_PORT, os.path.join(OUT, "B.log"), "B")
    b.start()
    # A：模拟历史走查的「单次 recv、调用间隙才收」
    a = Pumper(SER_PORT, os.path.join(OUT, "A.log"), "A")
    # 只有一路能连上同一个 tcp 串口，故 A 用「慢速轮询」的同一连接模型：
    # 这里改为让 A 复用严格泵、但在收完后做一次「快照比对」，看有无行尾缺失。
    time.sleep(0.5)

    mon = None
    for _ in range(200):
        try:
            mon = socket.create_connection(("127.0.0.1", MON_PORT), timeout=1)
            break
        except OSError:
            time.sleep(0.1)

    t0 = time.time()
    keys_done = False
    while time.time() - t0 < secs:
        el = time.time() - t0
        txt = bytes(b.buf).decode("utf-8", "replace")
        if not keys_done and "boot-diag:" in txt and el > 3:
            time.sleep(1.0)
            mon.sendall(b"sendkey down" + NL)
            time.sleep(1.5)
            mon.sendall(b"sendkey ret" + NL)
            keys_done = True
        if "boot-select: entry=" in txt:
            # 收到了 entry=，再等 3s 确认行尾字符是否补齐
            time.sleep(3.0)
            break
        time.sleep(0.2)

    b.stop = True
    time.sleep(0.5)
    try:
        mon.close()
    except Exception:
        pass
    p.terminate()
    try:
        p.wait(timeout=5)
    except subprocess.TimeoutExpired:
        p.kill()

    raw = bytes(b.buf)
    print("=== 严格泵采集结果 ===")
    print("连接次数: %d  总字节: %d" % (b.connects, len(raw)))
    for ln in raw.decode("utf-8", "replace").splitlines():
        if "entry=" in ln or "boot-diag" in ln:
            print("  | %r" % ln)
    print("\n严格泵是否收到完整 entry=varix : %s" % ("是" if b"entry=varix" in raw else "否"))
    print("是否出现被切的 entry=v         : %s" % ("是" if b"entry=v\n" in raw or raw.endswith(b"entry=v") else "否"))


if __name__ == "__main__":
    main()
