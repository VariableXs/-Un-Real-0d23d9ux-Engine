"""任务20 实机探针驱动：等 display-probe: scroll begin → 连续 screendump ×10
→ 逐帧撕裂分析（左半屏应恰好一条连续纯蓝带、带外全黑）→ 等 verdict。

用法：python _attic/display-probe-drive.py [serial_log] [monitor_port] [shot_prefix]
"""
import sys
import time
import socket

LOG = sys.argv[1] if len(sys.argv) > 1 else "kernel/qemu-serial-shared.log"
PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 14447
PREFIX = sys.argv[3] if len(sys.argv) > 3 else "kernel/display-shot"

BLUE = (0x20, 0x80, 0xF0)
BAND_H = 40


def hmp_screendump(path: str) -> None:
    s = socket.create_connection(("127.0.0.1", PORT), timeout=5)
    s.settimeout(2)
    time.sleep(0.25)
    try:
        s.recv(65536)
    except OSError:
        pass
    s.sendall(f"screendump {path}\n".encode())
    time.sleep(0.35)
    try:
        s.recv(65536)
    except OSError:
        pass
    s.close()


def wait_marker(marker: str, timeout_s: int) -> bool:
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


def analyze(path: str) -> str:
    """左半屏判定：恰好一个连续纯蓝行段（高 40±2），其余行全黑，无部分蓝行。"""
    data = open(path, "rb").read()
    hdr_end = data.index(b"255\n") + 4
    body = data[hdr_end:]
    w = 1280
    h = len(body) // (w * 3)
    half = w // 2
    blue_rows = []
    partial = 0
    for y in range(h):
        row = body[y * w * 3 : (y + 1) * w * 3]
        left = row[: half * 3]
        n_blue = 0
        n_other = 0
        for x in range(half):
            r, g, b = left[x * 3], left[x * 3 + 1], left[x * 3 + 2]
            if (r, g, b) == BLUE:
                n_blue += 1
            elif (r, g, b) != (0, 0, 0):
                n_other += 1
        if n_blue == half:
            blue_rows.append(y)
        elif n_blue > 0:
            partial += 1  # 部分蓝行 = 行内撕裂
        if n_other > 0:
            partial += 1  # 非黑非蓝 = 背景残留（不该出现在滚动域）
    if partial:
        return "torn-partial-rows"
    if not blue_rows:
        return "torn-no-band"
    # 连续段检测
    segs = []
    start = blue_rows[0]
    prev = blue_rows[0]
    for y in blue_rows[1:]:
        if y != prev + 1:
            segs.append((start, prev))
            start = y
        prev = y
    segs.append((start, prev))
    if len(segs) != 1:
        return f"torn-{len(segs)}-bands"
    height = segs[0][1] - segs[0][0] + 1
    if abs(height - BAND_H) > 2:
        return f"torn-height-{height}"
    return "clean"


def main() -> int:
    print("[drive] waiting for display-probe: scroll begin ...", flush=True)
    if not wait_marker("display-probe: scroll begin", 240):
        print("[drive] TIMEOUT waiting for scroll begin")
        return 1
    # 持久连接连拍：10 帧 × ~0.2s ≈ 2s，全部落在 3.6s 滚动窗口内。
    s = socket.create_connection(("127.0.0.1", PORT), timeout=5)
    s.settimeout(0.4)
    time.sleep(0.25)
    try:
        s.recv(65536)
    except OSError:
        pass
    verdicts = []
    for i in range(10):
        path = f"{PREFIX}-{i}.ppm"
        s.sendall(f"screendump {path}\n".encode())
        time.sleep(0.15)
        try:
            s.recv(65536)
        except OSError:
            pass
        v = analyze(path)
        verdicts.append(v)
        print(f"[drive] shot {i}: {v}", flush=True)
    s.close()
    clean = sum(1 for v in verdicts if v == "clean")
    torn = [v for v in verdicts if v != "clean"]
    print(f"[drive] clean={clean}/10 torn={torn}")
    ok = wait_marker("display-probe: verdict=ok", 120)
    print(f"[drive] kernel verdict line seen: {ok}")
    if not ok:
        return 1
    return 0 if clean >= 8 else 2


if __name__ == "__main__":
    sys.exit(main())
