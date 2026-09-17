"""任务19 实机探针驱动：等内核 input-probe: live 标记 → HMP 注入键鼠 → 收 verdict。

用法：python _attic/input-probe-drive.py [serial_log] [monitor_port]
"""
import sys
import time
import socket

LOG = sys.argv[1] if len(sys.argv) > 1 else "kernel/qemu-serial-shared.log"
PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 14447


def hmp(cmd: str) -> str:
    """连 HMP monitor 发一条命令并回读。"""
    s = socket.create_connection(("127.0.0.1", PORT), timeout=5)
    s.settimeout(2)
    time.sleep(0.3)
    try:
        s.recv(65536)  # 吃掉 banner
    except OSError:
        pass
    s.sendall(cmd.encode() + b"\n")
    time.sleep(0.4)
    out = b""
    try:
        while True:
            chunk = s.recv(65536)
            if not chunk:
                break
            out += chunk
    except OSError:
        pass
    s.close()
    return out.decode("utf-8", "replace")


def wait_marker(marker: str, timeout_s: int) -> bool:
    """轮询串口日志等标记出现。"""
    end = time.time() + timeout_s
    while time.time() < end:
        try:
            with open(LOG, "r", errors="replace") as f:
                if marker in f.read():
                    return True
        except OSError:
            pass
        time.sleep(1)
    return False


def main() -> int:
    print("[drive] waiting for input-probe: live ...", flush=True)
    if not wait_marker("input-probe: live", 240):
        print("[drive] TIMEOUT waiting for live marker")
        return 1
    print("[drive] live; injecting keys/mouse", flush=True)

    seq = [
        ("sendkey down", 0.8),
        ("sendkey up", 0.8),
        ("sendkey ret", 0.8),
        ("mouse_move 40 30", 1.0),
        ("mouse_move -30 -20", 1.0),
        ("mouse_button 1", 0.8),
        ("mouse_button 0", 0.8),
    ]
    for cmd, gap in seq:
        hmp(cmd)
        time.sleep(gap)
        print(f"[drive] sent: {cmd}", flush=True)

    print("[drive] waiting for verdict ...", flush=True)
    end = time.time() + 60
    while time.time() < end:
        try:
            with open(LOG, "r", errors="replace") as f:
                text = f.read()
            idx = text.rfind("input-probe: verdict=")
            if idx >= 0:
                line = text[idx : text.index("\n", idx)]
                print("[drive] " + line)
                return 0 if "verdict=true" in line else 2
        except OSError:
            pass
        time.sleep(1)
    print("[drive] TIMEOUT waiting for verdict")
    return 1


if __name__ == "__main__":
    sys.exit(main())
