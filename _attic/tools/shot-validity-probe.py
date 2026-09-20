#!/usr/bin/env python3
"""截图有效性对照：证明 `-display none` 下 HMP `screendump` 抓的是**冻结帧**。

原理：OVMF 只在真正需要刷新时（打印机文本）推帧。`-display none` 下
没有显示器在消费，内核自己画的菜单（写线性帧缓冲）不会触发 VGA 重绘，
于是 framebuffer 冻在 OVMF 最后绘制的画面。判定法：
  对照组 = **不接磁盘镜像**（OVMF 直接落到 UEFI Shell / 无引导），
           若能截出与「有盘」场景**完全同一张**菜单图，则证明截图不反映当下。

用法：python shot-validity-probe.py [秒数]
证据：_attic/acceptance-menu/shot-validity/*.png
"""
import hashlib
import os
import socket
import subprocess
import sys
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ATTIC = os.path.join(ROOT, "_attic")
QEMU = r"C:\Program Files\qemu\qemu-system-x86_64"
CODE = os.path.join(ATTIC, "edk2-x86_64-code.fd")
VARS = os.path.join(ATTIC, "edk2-vars-shotprobe.fd")
IMG = os.path.join(ATTIC, "varix-uefi.img")
OUT = os.path.join(ATTIC, "acceptance-menu", "shot-validity")
MON_PORT = 14951
SER_PORT = 14952
NL = bytes([10])


def pick():
    s = socket.socket(); s.bind(("127.0.0.1", 0)); p = s.getsockname()[1]; s.close()
    return p


def run(tag, with_disk, secs, shot_at):
    os.makedirs(OUT, exist_ok=True)
    if not os.path.exists(VARS):
        with open(VARS, "wb") as f:
            f.write(b"\x00" * (256 * 1024))
    cmd = [
        QEMU, "-machine", "q35", "-m", "2048", "-smp", "2", "-cpu", "max",
        "-drive", "if=pflash,format=raw,file=%s,unit=0,readonly=on" % CODE,
        "-drive", "if=pflash,format=raw,file=%s,unit=1" % VARS,
    ]
    if with_disk:
        cmd += ["-drive", "file=%s,format=raw,if=ide,index=0" % IMG, "-boot", "order=c"]
    cmd += [
        "-serial", "tcp:127.0.0.1:%d,server,nowait" % pick(),
        "-monitor", "tcp:127.0.0.1:%d,server,nowait" % MON_PORT,
        "-display", "none", "-no-reboot",
    ]
    print("[%s] with_disk=%s" % (tag, with_disk))
    p = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    mon = None
    for _ in range(200):
        try:
            mon = socket.create_connection(("127.0.0.1", MON_PORT), timeout=1)
            break
        except OSError:
            time.sleep(0.1)
    if mon is None:
        print("[%s] monitor 连不上" % tag)
        p.kill(); return None
    mon.settimeout(None)

    def shot(name):
        mon.sendall(b"screendump " + os.path.join(OUT, name).replace("\\", "/").encode() + NL)
        time.sleep(0.8)

    t0 = time.time()
    while time.time() - t0 < shot_at:
        time.sleep(0.2)
    shot("%s-t%.0fs.png" % (tag, shot_at))
    # 再等一截抓第二张，看是否与第一张逐字节相同（冻结的证据）
    time.sleep(6)
    shot("%s-t%.0fs.png" % (tag, shot_at + 6))
    try:
        mon.close()
    except Exception:
        pass
    p.terminate()
    try:
        p.wait(timeout=5)
    except subprocess.TimeoutExpired:
        p.kill()

    out = {}
    for f in sorted(os.listdir(OUT)):
        if f.startswith(tag):
            fp = os.path.join(OUT, f)
            out[f] = (os.path.getsize(fp), hashlib.sha256(open(fp, "rb").read()).hexdigest()[:16])
    for k, v in out.items():
        print("   %-28s %d bytes  sha256=%s" % (k, v[0], v[1]))
    return out


if __name__ == "__main__":
    shot_at = int(sys.argv[1]) if len(sys.argv) > 1 else 20
    print("=== 断言：截图为冻结帧（与当下内核状态无关）===")
    a = run("nodsik", False, shot_at + 12, shot_at)
    b = run("withdisk", True, shot_at + 12, shot_at)
    print("\n=== 结论 ===")
    if a and b:
        sa = {v[1] for v in a.values()}
        sb = {v[1] for v in b.values()}
        if sa & sb:
            print("无盘组与有盘组存在**完全相同**的截图 (%s)" % (sa & sb))
            print("→ 铁证：screendump 抓的是冻结帧，不能作为内核渲染状态的判据。")
        else:
            print("两组截图不同，截图可能有效（需进一步核实）。")
        for grp, name in ((a, "无盘"), (b, "有盘")):
            hs = {v[1] for v in grp.values()}
            print("  %s组内部是否逐字节相同: %s" % (name, len(hs) == 1))
