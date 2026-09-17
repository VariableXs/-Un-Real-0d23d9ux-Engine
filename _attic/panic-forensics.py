#!/usr/bin/env python3
"""panic 现场深度取证：读 panic handler 帧区 + 提取返回地址链。"""
import socket
import struct
import sys

MON_PORT = 14661


def hmp(cmd):
    c = socket.create_connection(("127.0.0.1", MON_PORT), timeout=5)
    c.settimeout(5)
    buf = b""
    while b"(qemu)" not in buf:
        buf += c.recv(4096)
    c.sendall(cmd.encode() + b"\n")
    out = b""
    while b"(qemu)" not in out:
        out += c.recv(65536)
    c.close()
    return out.decode(errors="replace")


# 读 RSP 下方 panic 帧 + 上方（两段）
lo = 0x1FF99800
hi = 0x1FF99E60
vals = []
for base in range(lo, hi, 0x80):
    out = hmp("xp /16gx 0x%x" % base)
    for line in out.splitlines():
        if ":" not in line or line.startswith(("QEMU", "\x1b")):
            continue
        for tok in line.split(":")[1].split():
            tok = tok.strip().rstrip(",")
            if tok.startswith("0x"):
                try:
                    vals.append((int(line.split(":")[0].strip(), 16) + 8 * len(vals[:0]), int(tok, 16)))
                except ValueError:
                    pass

# 重读一遍带地址配对
pairs = []
for base in range(lo, hi, 0x80):
    out = hmp("xp /16gx 0x%x" % base)
    for line in out.splitlines():
        if ":" not in line or line.startswith("QEMU"):
            continue
        addr_part = line.split(":")[0].strip()
        try:
            a0 = int(addr_part, 16)
        except ValueError:
            continue
        idx = 0
        for tok in line.split(":")[1].split():
            tok = tok.strip().rstrip(",")
            if tok.startswith("0x"):
                try:
                    pairs.append((a0 + idx * 8, int(tok, 16)))
                    idx += 1
                except ValueError:
                    pass

print("=== 内核文本返回地址（0xffffffff8xxxxxxx）===")
data = open("kernel/target/x86_64-unknown-none/release/varix", "rb").read()
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

seen = set()
for a, v in pairs:
    if 0xFFFFFFFF80001000 <= v < 0xFFFFFFFF80A00000:
        best = None
        for sv, sn, ss in syms:
            if sv <= v:
                best = (sv, sn, ss)
        if best and best[1] not in seen:
            seen.add(best[1])
            print("栈 %#x: %#x -> %s+%#x" % (a, v, best[1], v - best[0]))

print()
print("=== ASCII 字符串（panic 消息证据）===")
for a, v in pairs:
    bs = struct.pack("<Q", v & 0xFFFFFFFFFFFFFFFF)
    s = "".join(chr(b) if 32 <= b < 127 else "." for b in bs)
    if s.strip(".") and any(ch.isalpha() for ch in s):
        print("栈 %#x: %r" % (a, s))
