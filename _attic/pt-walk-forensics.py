#!/usr/bin/env python3
"""notepad #PF 取证：HMP 手撕 CR3 页表，读 IAT 页(0x140002000)与 thunk 页(0x70000000) 的 PTE。
ERR=0x14 (user+fetch+not-present), RIP=0 → 用户 call [IAT] 读到 0。
"""
import subprocess, time, socket, struct, sys, re

SERIAL = "_attic/p41p48-serial.log"
MON = 14661

def read_phys(c, pa, n=8):
    """xp /<n>bx <addr> 读物理内存字节。"""
    c.sendall(f"xp /{n}bx {pa:#x}\n".encode())
    out = b""
    while b"(qemu)" not in out:
        out += c.recv(65536)
    txt = out.decode(errors="replace")
    vals = []
    for line in txt.splitlines():
        for tok in line.split():
            if tok.startswith("0x"):
                try:
                    vals.append(int(tok, 16) & 0xFF)
                except ValueError:
                    pass
    # 去掉命令回显里的地址数字（取最后 n 个）
    return bytes(vals[-n:]) if len(vals) >= n else b"\0" * n

def walk(c, cr3, va):
    idxs = [(va >> 39) & 0x1FF, (va >> 30) & 0x1FF, (va >> 21) & 0x1FF, (va >> 12) & 0x1FF]
    pa = cr3 & ~0xFFF
    for level, idx in enumerate(idxs[:3]):
        raw = read_phys(c, pa + idx * 8)
        ent = struct.unpack("<Q", raw)[0]
        print(f"  L{level+1}[{idx}] @ {pa + idx*8:#x} = {ent:#018x}")
        if not ent & 1:
            print(f"  -> L{level+1} NOT PRESENT")
            return None
        pa = ent & 0x000FFFFFFFFFF000
    raw = read_phys(c, pa + idxs[3] * 8)
    pte = struct.unpack("<Q", raw)[0]
    print(f"  L4 PT[{idxs[3]}] @ {pa + idxs[3]*8:#x} = {pte:#018x}")
    if not pte & 1:
        print("  -> PTE NOT PRESENT")
        return None
    return pte & 0x000FFFFFFFFFF000

subprocess.run(["taskkill", "/F", "/IM", "qemu-system-x86_64.exe"], capture_output=True)
time.sleep(2)
with open(SERIAL, "w") as f:
    f.write("")
proc = subprocess.Popen([
    "qemu-system-x86_64", "-cdrom", "varix-qemu.iso",
    "-serial", "file:" + SERIAL, "-monitor", f"tcp:127.0.0.1:{MON},server,nowait",
    "-m", "1024", "-no-reboot", "-no-shutdown",
], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

cr3 = None
for i in range(240):
    time.sleep(2)
    log = open(SERIAL, encoding="utf-8", errors="replace").read()
    if "fatal exception" in log and "notepad spawned" in log:
        time.sleep(1)
        # 从日志抓最新 CR3
        c = socket.create_connection(("127.0.0.1", MON), timeout=5)
        c.settimeout(5)
        buf = b""
        while b"(qemu)" not in buf:
            buf += c.recv(4096)
        c.sendall(b"info registers\n")
        regs = b""
        while b"(qemu)" not in regs:
            regs += c.recv(65536)
        m = re.search(r"CR3=([0-9a-fA-F]+)", regs.decode(errors="replace"))
        if m:
            cr3 = int(m.group(1), 16)
        print(f"CR3={cr3:#x}")
        for va, name in ((0x140002104, "IAT[0] page"), (0x140002124, "IAT[RegisterClass] page"),
                         (0x70000000, "thunk page"), (0x140001000, ".text entry page")):
            print(f"{name} va={va:#x}:")
            phys = walk(c, cr3, va)
            if phys:
                data = read_phys(c, phys + (va & 0xFFF), 16)
                print(f"  frame={phys:#x} first16={data.hex(' ')}")
        c.close()
        break
subprocess.run(["taskkill", "/F", "/IM", "qemu-system-x86_64.exe"], capture_output=True)
