#!/usr/bin/env python3
r"""PTE 铁证探针：冻结 VM 后逐级读 AP 栈页 VA 0xffffffff8044f040 的页表链，
并抓 BSP/AP 完整寄存器（重点 EFER.NXE）。

判定：若栈页 PTE bit63(NX)=1 且 AP EFER 无 NXE(0x800) → err=0xa 的
RSVD #PF 根因坐实（NXE=0 时 NX 位=保留位违例）。
同时读内核代码页 VA 0xffffffff800237b0 的 PTE 作对照（预期无 NX）。
"""
import os
import re
import socket
import subprocess
import time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
ATTIC = ROOT + r"\_attic"
DISK = ATTIC + r"\uefi-esp.vhd"
SERIAL = ATTIC + r"\pte-probe-serial.log"
ERRLOG = ATTIC + r"\pte-probe-stderr.log"
MON_PORT = 14465
QEMU = r"C:\Program Files\qemu\qemu-system-x86_64"
EDK2 = ATTIC + r"\edk2-x86_64-code.fd"

STACK_VA = 0xFFFFFFFF8044F040   # AP 死亡现场：push rbp 故障地址
CODE_VA = 0xFFFFFFFF800237B0    # varix_ap_entry 取指处（对照）


def start_qemu():
    return subprocess.Popen(
        [
            QEMU,
            "-M", "pc", "-m", "512M",
            "-smp", "2",
            "-cpu", "max",
            "-drive", f"if=pflash,format=raw,file={EDK2},unit=0",
            "-drive", f"file={DISK},format=vpc,if=ide,index=0",
            "-boot", "order=c",
            "-serial", f"file:{SERIAL}",
            "-display", "none", "-no-reboot", "-no-shutdown",
            "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait",
        ],
        stdout=open(ERRLOG, "wb"), stderr=subprocess.STDOUT,
    )


def monitor_cmd(sock, cmd, wait=2.5):
    sock.sendall((cmd + "\n").encode())
    sock.settimeout(wait)
    buf = b""
    t0 = time.time()
    while time.time() - t0 < wait:
        try:
            chunk = sock.recv(65536)
            if not chunk:
                break
            buf += chunk
            if buf.rstrip().endswith(b"(qemu)"):
                break
        except socket.timeout:
            break
    return buf.decode("gbk", "replace")


def read_u64(sock, pa):
    """xp /1xg 读一个物理地址处的 u64，解析出值。"""
    out = monitor_cmd(sock, f"xp /1xg 0x{pa:x}")
    m = re.search(r"0x([0-9a-fA-F]{16})", out)
    if not m:
        m2 = re.search(r"([0-9a-fA-F]{16})\s*$", out.strip().splitlines()[-1] if out.strip() else "")
        if not m2:
            return None, out
        return int(m2.group(1), 16), out
    return int(m.group(1), 16), out


def decode(entry, level):
    if entry is None:
        return "READ-FAILED"
    bits = []
    if entry & 1:
        bits.append("P")
    if entry & 2:
        bits.append("W")
    if entry & (1 << 63):
        bits.append("NX")
    if entry & (1 << 7) and level > 1:
        bits.append("PS(2M)")
    addr = entry & 0x000F_FFFF_FFFF_F000
    return f"0x{entry:016x} -> PA 0x{addr:x} [{'|'.join(bits) if bits else 'CLEAN-ZERO'}]"


def walk(sock, va, cr3, label):
    print(f"===== {label}: VA 0x{va:016x} (CR3=0x{cr3:x}) =====")
    idxs = [(va >> 39) & 0x1FF, (va >> 30) & 0x1FF, (va >> 21) & 0x1FF, (va >> 12) & 0x1FF]
    names = ["PML4", "PDPT", "PD", "PT"]
    pa = cr3
    for level, idx in enumerate(idxs, start=1):
        addr = pa + idx * 8
        entry, raw = read_u64(sock, addr)
        print(f"[{names[level-1]}[{idx:#x}] @ PA 0x{addr:x}] {decode(entry, level)}")
        if entry is None or not (entry & 1):
            print(f"  !! {names[level-1]} 项 not-present，链条到此为止")
            return
        pa = entry & 0x000F_FFFF_FFFF_F000
    print(f"  => 栈/代码页物理帧: 0x{pa:x}")


def main():
    if os.path.exists(SERIAL):
        os.remove(SERIAL)
    proc = start_qemu()
    print(f"[pte-probe] QEMU pid={proc.pid}，等待 marker…")
    marker = "clock: source=tsc"
    deadline = time.time() + 180
    hit = False
    while time.time() < deadline:
        time.sleep(3)
        if proc.poll() is not None:
            print(f"[pte-probe] QEMU 提前退出 rc={proc.returncode}")
            return
        if os.path.exists(SERIAL):
            tail = open(SERIAL, "r", errors="replace").read()
            if marker in tail:
                hit = True
                break
    print(f"[pte-probe] marker hit={hit}，8s 后冻结")
    time.sleep(8)

    try:
        s = socket.create_connection(("127.0.0.1", MON_PORT), timeout=5)
    except OSError as e:
        print(f"[pte-probe] 监视器连接失败: {e}")
        proc.kill()
        return
    s.settimeout(3)
    try:
        s.recv(65536)
    except OSError:
        pass

    out = monitor_cmd(s, "info registers -a", wait=4)
    print("---- info registers -a ----")
    print(out[:16000])

    # CR3 从寄存器输出里重新确认（两个核都可能列出）
    cr3s = re.findall(r"CR3=([0-9a-f]{16})", out)
    cr3 = int(cr3s[0], 16) if cr3s else 0x1DF2E000
    print(f"[pte-probe] 使用 CR3=0x{cr3:x}（从寄存器快照提取）")

    walk(s, STACK_VA, cr3, "AP 栈页")
    walk(s, CODE_VA, cr3, "内核代码页(对照)")

    monitor_cmd(s, "quit", wait=1.5)
    try:
        proc.wait(timeout=6)
    except subprocess.TimeoutExpired:
        proc.kill()
    print("[pte-probe] done")


if __name__ == "__main__":
    main()
