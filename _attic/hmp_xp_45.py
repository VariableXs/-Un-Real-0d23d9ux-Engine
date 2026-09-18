# -*- coding: utf-8 -*-
"""HMP over TCP 查询工具：xp 查 QEMU 虚拟机物理内存（Windows 无 AF_UNIX 的替代）。
用法: python hmp_xp.py <addr> [addr...]   如: python hmp_xp.py 0x1ef26000 0x1ef27000
"""
import socket
import sys
import time

HOST, PORT = ("127.0.0.1", 14445)


def main() -> int:
    addrs = sys.argv[1:]
    if not addrs:
        print("usage: hmp_xp.py <addr> ...")
        return 1
    s = socket.create_connection((HOST, PORT), timeout=5)
    s.settimeout(1.5)

    def drain():
        out = b""
        try:
            while True:
                chunk = s.recv(65536)
                if not chunk:
                    break
                out += chunk
        except socket.timeout:
            pass
        return out.decode("utf-8", errors="replace")

    drain()  # banner
    for a in addrs:
        s.sendall(f"xp /16wx {a}\n".encode())
        time.sleep(0.4)
        print(f"--- {a} ---")
        print(drain().strip())
    s.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
