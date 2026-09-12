#!/usr/bin/env python3
"""QEMU monitor 便捷查询工具。

用法:
    python tools/qmon.py <port> "<cmd1>" ["<cmd2>" ...]

连接 127.0.0.1:<port> 上的 QEMU HMP monitor（-monitor tcp:...,server,nowait），
发送命令并打印输出。自动剥离 readline 回显与 ANSI 转义序列。

注意: monitor 的 `xp` 只能看已映射地址；致命异常优先看串口日志与
`info registers` / `info status` / CR2。
"""
import re
import socket
import sys


ANSI_RE = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]")


def strip_noise(data: str) -> str:
    data = ANSI_RE.sub("", data)
    # readline 回显: 命令本身会被 echo 回来，去掉回车噪声
    return data.replace("\r", "")


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    port = int(sys.argv[1])
    cmds = sys.argv[2:]

    sock = socket.create_connection(("127.0.0.1", port), timeout=5)
    sock.settimeout(1.5)

    def drain() -> str:
        chunks = []
        try:
            while True:
                b = sock.recv(65536)
                if not b:
                    break
                chunks.append(b.decode("utf-8", "replace"))
        except socket.timeout:
            pass
        return "".join(chunks)

    drain()  # 吃掉初始 (qemu) 提示符
    for cmd in cmds:
        sock.sendall(cmd.encode("utf-8") + b"\n")
        out = drain()
        text = strip_noise(out)
        # 去掉被 echo 的命令行本身
        text = text.replace(cmd, "", 1)
        print(f"=== {cmd} ===")
        print(text.strip())
    sock.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
