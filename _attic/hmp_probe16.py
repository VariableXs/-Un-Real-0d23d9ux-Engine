# -*- coding: utf-8 -*-
"""QEMU HMP monitor 物理内存核验：SQ/CQ 在 QEMU 视角的内容 + mtree。"""
import socket, subprocess, time, os

SOCK = "/tmp/hmp16.sock"
if os.path.exists(SOCK):
    os.remove(SOCK)

cmd = [
    "qemu-system-x86_64",
    "-drive", "format=raw,file=varix.img",
    "-drive", "file=nvme0.img,if=none,id=nvme0,format=raw",
    "-device", "nvme,serial=VARIX16,drive=nvme0",
    "-serial", "file:kernel/qemu-serial-t16m.log",
    "-display", "none", "-no-reboot", "-m", "512M", "-M", "q35",
    "-monitor", f"unix:{SOCK},server,nowait",
]
proc = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
time.sleep(14)  # 等 kernel 跑完 identify 超时两轮

s = socket.socket(socket.AF_UNIX)
s.connect(SOCK)
s.settimeout(2)
time.sleep(0.6)
try:
    print(s.recv(8192).decode(errors="replace")[-200:])
except socket.timeout:
    pass


def hmp(c):
    s.sendall((c + "\n").encode())
    time.sleep(0.8)
    out = b""
    try:
        while True:
            chunk = s.recv(65536)
            if not chunk:
                break
            out += chunk
    except socket.timeout:
        pass
    print(">>>", c)
    print(out.decode(errors="replace"))


# 关键帧：init 用 DmaBuckets 顺序 alloc（sq/cq 在 enable 内第 1/2 帧）
hmp("xp /8wx 0x1ef01000")   # SQ 帧头（第 1 帧）
hmp("xp /8wx 0x1ef02000")   # CQ 帧头（第 2 帧）
hmp("xp /8wx 0x1ef03000")   # 第 3 帧（identify 页）
hmp("info mtree")
proc.kill()
proc.wait()
