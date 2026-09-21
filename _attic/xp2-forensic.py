# -*- coding: utf-8 -*-
"""一体化取证：启动 QEMU → 等超时发生 → monitor xp 双侧内存对照 → 优雅退出。"""
import re
import socket
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main")
ATTIC = ROOT / "_attic"
ISO = ATTIC / "varix-xhci.iso"
SERIAL = ATTIC / "xp2-serial.log"
MON_PORT = 14810


def main():
    import os
    if SERIAL.exists():
        os.remove(SERIAL)
    proc = subprocess.Popen(
        [
            "qemu-system-x86_64", "-machine", "q35",
            "-cdrom", str(ISO),
            "-device", "nec-usb-xhci,id=xhci",
            "-device", "usb-kbd,bus=xhci.0",
            "-device", "usb-mouse,bus=xhci.0",
            "-serial", f"file:{SERIAL}",
            "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait",
            "-m", "1024",
        ],
        cwd=str(ROOT),
    )
    # 等到第一个 cmd timeout 出现（驱动卡在等完成事件）。
    erdp = None
    for _ in range(90):
        time.sleep(1)
        text = SERIAL.read_text(encoding="utf-8", errors="replace") if SERIAL.exists() else ""
        m = re.search(r"erdp=0x([0-9a-f]+)", text)
        if m:
            erdp = int(m.group(1), 16)
            break
    if erdp is None:
        print("NO TIMEOUT OBSERVED")
        proc.terminate()
        return 2
    evt = erdp & ~0xFFF
    cmdf = evt - 0x1000
    print(f"TIMEOUT SEEN: erdp={erdp:#x} evt_frame={evt:#x} cmd_frame={cmdf:#x}")
    time.sleep(2)  # 让第二次超时也发生（多取几份现场）

    sock = socket.create_connection(("127.0.0.1", MON_PORT), timeout=5)
    sock.settimeout(3)

    def drain():
        time.sleep(0.5)
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

    drain()

    def cmd(line):
        sock.sendall((line + "\n").encode())
        return drain()

    def words_of(text):
        words = []
        for line in text.splitlines():
            for tok in line.split():
                if tok.startswith("0x") and len(tok) == 10:
                    try:
                        words.append(int(tok, 16))
                    except ValueError:
                        pass
        return words

    print("== EVENT RING FRAME (QEMU xp): ERST@+0, ring@+0x40 ==")
    for off in (0, 0x20, 0x40, 0x60, 0x80, 0xA0):
        out = cmd("xp /4wx %#x" % (evt + off))
        w = words_of(out)
        if len(w) >= 4:
            print("  +0x%03x: %08x %08x %08x %08x" % (off, w[0], w[1], w[2], w[3]))
    print("== CMD RING FRAME (QEMU xp): slots 0-3 ==")
    for off in (0, 0x20, 0x40, 0x60):
        out = cmd("xp /4wx %#x" % (cmdf + off))
        w = words_of(out)
        if len(w) >= 4:
            print("  +0x%03x: %08x %08x %08x %08x" % (off, w[0], w[1], w[2], w[3]))

    cmd("quit")
    try:
        proc.wait(timeout=10)
    except Exception:
        proc.kill()
    sock.close()

    print("== DRIVER SERIAL (forensic lines) ==")
    text = SERIAL.read_text(encoding="utf-8", errors="replace")
    for line in text.splitlines():
        if "cmd timeout" in line or "forensic" in line or "evt_slot" in line or "cmd_slot" in line:
            print(" ", line)
    return 0


if __name__ == "__main__":
    sys.exit(main())
