#!/usr/bin/env python3
"""UEFI 引导 A/B 判定器。

同一个 OVMF、同一条命令，只换磁盘镜像：
  A = varix.img        （MBR + FAT32，BIOS 走查已在用的成熟产物）
  B = _attic/varix-uefi.img（GPT + ESP，我新写的）
谁能让内核起来，就说明问题在「分区表形态」而不是「FAT32/文件内容」。

用法： python uefi-ab-probe.py [A|B|AB（默认）] [秒数（默认 40）]
"""
import os
import socket
import subprocess
import sys
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
QEMU = "qemu-system-x86_64"
CODE = os.path.join(ROOT, "_attic", "edk2-x86_64-code.fd")
NL = bytes([10])

IMG_A = os.path.join(ROOT, "varix.img")
IMG_B = os.path.join(ROOT, "_attic", "varix-uefi.img")

# 判据：串口出现这些行之一 = 内核真的起来了
KERNEL_ALIVE = [
    b"varix",            # 内核自报
    b"cmdline:",
    b"boot-diag:",
    b"kbd-init",
    b"menu",
    b"boot-select",
]


def pick_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


def run(tag, img, secs):
    port = pick_port()
    vars_fd = os.path.join(ROOT, "_attic", "edk2-vars-%s.fd" % tag)
    if not os.path.exists(vars_fd):
        with open(vars_fd, "wb") as f:
            f.write(b"\x00" * (256 * 1024))
    cmd = [
        QEMU, "-machine", "q35", "-m", "1024", "-smp", "2", "-cpu", "max",
        "-drive", "if=pflash,format=raw,unit=0,readonly=on,file=%s" % CODE,
        "-drive", "if=pflash,format=raw,unit=1,file=%s" % vars_fd,
        "-drive", "file=%s,format=raw,if=ide,index=0" % img,
        "-boot", "order=c",
        "-serial", "tcp:127.0.0.1:%d,server,nowait" % port,
        "-monitor", "none", "-display", "none", "-no-reboot",
    ]
    print("[%s] img=%s port=%d" % (tag, os.path.basename(img), port))
    print("[%s] %s" % (tag, " ".join(cmd)))
    p = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    buf = bytearray()
    lock = threading.Lock()
    stop = {"v": False}

    def pump():
        s = None
        for _ in range(400):
            try:
                s = socket.create_connection(("127.0.0.1", port), timeout=0.5)
                break
            except OSError:
                time.sleep(0.05)
        if s is None:
            print("[%s] !! 串口连不上" % tag)
            return
        s.settimeout(None)
        while not stop["v"]:
            try:
                d = s.recv(4096)
            except OSError:
                break
            if not d:
                break
            with lock:
                buf.extend(d)

    t = threading.Thread(target=pump, daemon=True)
    # 先起进程，再连串口（QEMU 端口在进程起来前不在听）
    t.start()
    deadline = time.time() + secs
    alive_at = None
    while time.time() < deadline:
        time.sleep(0.5)
        with lock:
            snap = bytes(buf)
        if alive_at is None:
            low = snap.lower()
            if any(k in low for k in KERNEL_ALIVE):
                alive_at = time.time()
                print("[%s] >>> 内核起来了 (t=%.1fs)" % (tag, alive_at - start))
        if p.poll() is not None:
            break
    stop["v"] = True
    time.sleep(0.3)

    if alive_at is None:
        try:
            # 抓一屏看 OVMF 到底停在哪
            s = socket.create_connection(("127.0.0.1", port), timeout=1)
            s.settimeout(1.0)
            s.sendall(b"info status" + NL)
            time.sleep(0.3)
            try:
                d = s.recv(65536)
                with lock:
                    buf.extend(d)
            except OSError:
                pass
            s.close()
        except OSError:
            pass

    try:
        p.terminate()
    except Exception:
        pass
    try:
        p.wait(timeout=5)
    except subprocess.TimeoutExpired:
        p.kill()

    with lock:
        out = bytes(buf)
    txt = out.decode("utf-8", "replace")
    print("[%s] ---- 串口输出（%d 字节）----" % (tag, len(out)))
    for line in txt.splitlines():
        if line.strip():
            print("  | " + line.rstrip())
    print("[%s] 判定: %s" % (tag, "KERNEL-ALIVE" if alive_at else "NOT-ALIVE"))
    return bool(alive_at)


if __name__ == "__main__":
    which = sys.argv[1].upper() if len(sys.argv) > 1 else "AB"
    secs = int(sys.argv[2]) if len(sys.argv) > 2 else 40
    start = time.time()
    res = {}
    if "A" in which:
        res["A(MBR)"] = run("A", IMG_A, secs)
    if "B" in which:
        res["B(GPT)"] = run("B", IMG_B, secs)
    print("\n=== 汇总 ===")
    for k, v in res.items():
        print("  %s : %s" % (k, "内核起来了" if v else "没起来"))
