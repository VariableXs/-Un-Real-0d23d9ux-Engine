# -*- coding: utf-8 -*-
"""QEMU monitor xp 物理内存取证：读事件环/命令环的 QEMU 侧真相。"""
import socket
import sys
import time

sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
sock.connect(("127.0.0.1", 14800))
sock.settimeout(3)


def drain():
    time.sleep(0.6)
    out = b""
    try:
        while True:
            chunk = sock.recv(65536)
            if not chunk:
                break
            out += chunk
    except socket.timeout:
        pass
    return out.decode("utf-8", "replace")


drain()  # 吞 banner


def cmd(line):
    sock.sendall((line + "\n").encode())
    return drain()


def u32s(text):
    """解析 xp /Nwx 输出：每行 [addr] 0x........ 0x..........."""
    words = []
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("0x") or (":" in line[:12] and "0x" in line):
            for tok in line.split():
                if tok.startswith("0x") and len(tok) == 10:
                    words.append(int(tok, 16))
    return words


# 事件环帧 + 命令环帧（本次 boot 的物理地址，来自串口取证行）
base_evt = 0x3F468000
base_cmd = 0x3F467000

print("== EVENT RING FRAME (0x%x): ERST@+0x00, ring@+0x40 ==" % base_evt)
for off in (0, 0x40, 0x60, 0x80, 0xA0):
    out = cmd("xp /4wx %#x" % (base_evt + off))
    words = u32s(out)
    if len(words) >= 4:
        print("  +0x%03x: %08x %08x %08x %08x" % (off, words[0], words[1], words[2], words[3]))

print("== CMD RING FRAME (0x%x): 前 4 槽 ==" % base_cmd)
for off in (0, 0x20, 0x40, 0x60):
    out = cmd("xp /4wx %#x" % (base_cmd + off))
    words = u32s(out)
    if len(words) >= 4:
        print("  +0x%03x: %08x %08x %08x %08x" % (off, words[0], words[1], words[2], words[3]))

sock.close()
