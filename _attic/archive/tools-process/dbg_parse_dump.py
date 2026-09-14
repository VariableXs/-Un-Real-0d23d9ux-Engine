# -*- coding: utf-8 -*-
"""Parse a minidump: for each thread, scan raw stack bytes and map return
addresses to modules (rough native stack)."""
import sys, io, struct
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")

path = sys.argv[1]
data = open(path, "rb").read()
sig, ver, nstreams, dirr, checksum, ts, flags = struct.unpack_from("<IIIIIIQ", data, 0)
assert sig == 0x504D444D  # MDMP

streams = {}
for i in range(nstreams):
    t, sz, rva = struct.unpack_from("<III", data, dirr + i * 12)
    streams[t] = (sz, rva)

modules = []
sz, rva = streams[4]
nrva = struct.unpack_from("<I", data, rva)[0]
for i in range(nrva):
    base = rva + 4 + i * 108
    mbase, msize, cks, tds, namerva = struct.unpack_from("<QIIII", data, base)
    nlen = struct.unpack_from("<I", data, namerva)[0]
    name = data[namerva + 4: namerva + 4 + nlen].decode("utf-16-le").rstrip("\x00")
    modules.append((mbase, msize, name.replace("\\", "/").split("/")[-1]))

def mod_of(addr):
    for mbase, msize, name in modules:
        if mbase <= addr < mbase + msize:
            return f"{name}+0x{addr - mbase:x}"
    return None

# MINIDUMP_THREAD (48 bytes): ThreadId(4) SuspendCount(4) PriorityClass(4)
# Priority(4) Teb(8) Stack.StartOfMemoryRange(8) Stack.Memory.DataSize(4)
# Stack.Memory.Rva(4) ExitStatus(4)
threads = []
sz, rva = streams[3]
n = struct.unpack_from("<I", data, rva)[0]
for i in range(n):
    base = rva + 4 + i * 48
    tid, susp = struct.unpack_from("<II", data, base)
    stk_start = struct.unpack_from("<Q", data, base + 24)[0]
    stk_size = struct.unpack_from("<I", data, base + 32)[0]
    stk_rva = struct.unpack_from("<I", data, base + 36)[0]
    threads.append((tid, susp, stk_start, stk_rva, stk_size))

print(f"{len(threads)} threads, {len(modules)} modules")
for tid, susp, s0, srva, ssz in threads:
    b = data[srva: srva + ssz] if srva else b""
    frames = []
    for off in range(0, max(0, min(len(b), 65536) - 8), 8):
        v = struct.unpack_from("<Q", b, off)[0]
        m = mod_of(v)
        if m and len(frames) < 40:
            frames.append(m)
    print(f"\n== TID {tid} suspend={susp} stackBase={hex(s0)} size={ssz}")
    for fr in frames[:14]:
        print("   ", fr)
