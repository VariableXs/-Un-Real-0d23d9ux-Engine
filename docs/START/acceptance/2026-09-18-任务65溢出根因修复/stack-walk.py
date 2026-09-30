#!/usr/bin/env python3
"""栈回溯诊断：HMP xp 读内核栈 -> 提取内核文本地址 -> 符号表对照。"""
import socket
import struct
import sys

ELF = "kernel/target/x86_64-unknown-none/release/varix"
STACK_VIRT = 0xFFFF80001FF99DF0  # 卡死现场 RSP
STACK_PHYS = STACK_VIRT - 0xFFFF800000000000  # 直映射偏移

# --- HMP xp 读栈 ---
c = socket.create_connection(("127.0.0.1", 14661), timeout=5)
c.settimeout(5)
buf = b""
while b"(qemu)" not in buf:
    buf += c.recv(4096)
c.sendall(b"xp /64gx 0x%x\n" % STACK_PHYS)
out = b""
while b"(qemu)" not in out:
    out += c.recv(65536)
c.close()
text = out.decode(errors="replace")

# --- 解析 xp 输出：每行 "000000001ff99df0: 0x.... 0x.... ..." ---
vals = []
for line in text.splitlines():
    if ":" not in line:
        continue
    for tok in line.split(":")[1].split():
        tok = tok.strip().rstrip(",")
        if tok.startswith("0x"):
            try:
                vals.append(int(tok, 16))
            except ValueError:
                pass

# --- ELF 符号表 ---
data = open(ELF, "rb").read()
e_shoff = struct.unpack_from("<Q", data, 0x28)[0]
e_shentsize, e_shnum, e_shstrndx = struct.unpack_from("<HHH", data, 0x3A)
secs = [struct.unpack_from("<IIQQQQIIQQ", data, e_shoff + i * e_shentsize) for i in range(e_shnum)]
shstr = secs[e_shstrndx][4]


def cstr(base, i):
    end = data.index(b"\0", base + i)
    return data[base + i:end].decode("latin1")


syms = []
for s in secs:
    if cstr(shstr, s[0]) == ".symtab" and s[1] == 2:
        strtab = secs[s[3]][4]
        for j in range(s[5] // 24):
            off = s[4] + j * 24
            st_name, st_info, st_other, st_shndx, st_value, st_size = struct.unpack_from("<IBBHQQ", data, off)
            if st_value >= 0xFFFFFFFF80000000:
                syms.append((st_value, cstr(strtab, st_name), st_size))
syms.sort()

KTEXT_LO = 0xFFFFFFFF80000000
KTEXT_HI = 0xFFFFFFFF80A00000


def nearest(addr):
    lo, hi = 0, len(syms) - 1
    best = None
    for v, n, sz in syms:
        if v <= addr:
            best = (v, n, sz)
    return best


print("=== 栈上内核地址回溯（RSP=%#x）===" % STACK_VIRT)
for v in vals:
    if KTEXT_LO + 0x1000 <= v < KTEXT_HI:
        b = nearest(v)
        if b:
            print("%#x  <- %s+%#x (size=%#x)" % (v, b[1], v - b[0], b[2]))
