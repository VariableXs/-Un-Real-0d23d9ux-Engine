#!/usr/bin/env python3
r"""QEMU 监视器取证：boot 后冻结 VM，dump 物理 0x8000（跳板落点）、
0xA000/0xB000/0xC000（恒等页表），并抓全部 vCPU 寄存器（AP 停在哪）。

判定：
  - xp 0x8000 处看到 FA FC 8C C8...（cli/cld/mov ax,cs）→ 跳板复制成功，问题在 IPI 投递；
  - 全 0 → install_trampoline 写错位置（hhdm 约定问题）；
  - info registers -a → AP 的 RIP/HALTED 状态判断 SIPI 是否送达。
"""
import os
import socket
import subprocess
import sys
import time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
ATTIC = ROOT + r"\_attic"
DISK = ATTIC + r"\uefi-esp.vhd"
SERIAL = ATTIC + r"\mon-probe-serial.log"
ERRLOG = ATTIC + r"\mon-probe-stderr.log"
MON_PORT = 14464
QEMU = r"C:\Program Files\qemu\qemu-system-x86_64"
EDK2 = ATTIC + r"\edk2-x86_64-code.fd"


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
    """发送一条 HMP 命令并收集到下一个提示符为止的输出。"""
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


def main():
    if os.path.exists(SERIAL):
        os.remove(SERIAL)
    proc = start_qemu()
    print(f"[probe] QEMU pid={proc.pid}，等待 smp 行出现…")
    marker = "clock: source=tsc"
    hit_wait = 8  # 命中后等几秒再冻结，确保卡死现场稳定
    deadline = time.time() + 180
    hit = False
    while time.time() < deadline:
        time.sleep(3)
        if proc.poll() is not None:
            print(f"[probe] QEMU 提前退出 rc={proc.returncode}")
            return
        if os.path.exists(SERIAL):
            tail = open(SERIAL, "r", errors="replace").read()
            if marker in tail:
                hit = True
                break
    print(f"[probe] marker={marker} hit={hit}，{hit_wait}s 后冻结取证")
    time.sleep(hit_wait)  # 卡死现场稳定

    try:
        s = socket.create_connection(("127.0.0.1", MON_PORT), timeout=5)
    except OSError as e:
        print(f"[probe] 监视器连接失败: {e}")
        proc.kill()
        return
    s.settimeout(3)
    try:
        s.recv(65536)  # 吃掉 banner
    except OSError:
        pass

    cmds = [
        "stop",
        "info status",
        "xp /320xb 0x8000",
        "xp /4xg 0x8110",
        "info registers -a",
    ]
    for c in cmds:
        out = monitor_cmd(s, c)
        print(f"---- {c} ----")
        print(out[:14000])
    monitor_cmd(s, "quit", wait=1.5)
    try:
        proc.wait(timeout=6)
    except subprocess.TimeoutExpired:
        proc.kill()
    print("[probe] done")


if __name__ == "__main__":
    main()
