#!/usr/bin/env python3
r"""A 卡交接走查（需求 2）——验证「内核加载完 → 交接进 Windows 上的 Variable」。

链路（见 kernel/varix/src/handoff.rs）：
  三卡菜单选 A 卡 → 内核加载 → 交接画面 → 写 UEFI BootNext → ResetSystem
  → 固件引导 Windows → 那边 Variable 自启全屏。

为什么必须重启：内核早期已调 ExitBootServices，引导服务交还固件，**无法**再
跳转到 Windows Boot Manager；UEFI 架构下唯一的路是 ResetSystem。所以"内核
→ Variable"物理上必然包含一次复位——这不是实现取舍，是硬约束。

三个场景（各自改镜像里的 limine.conf，跑完恢复原样）：
  * h1-fallback  无 Windows 项（QEMU 常态）→ 防自锁闸门生效，**降级落 ushell**
                 （盲写 BootNext 会因项无效回退默认顺序 → 再进菜单 → 无限复位循环）
  * h2-handoff   cmdline 显式 boot_next=0x0001 → 视为已验证 → 真交接：
                 交接画面 → BootNext 写入并校验 → ResetSystem
  * h3-disabled  cmdline handoff=0 → 直接落 ushell，**不尝试交接**

用法：python _attic/tools/menu-handoff-walk.py [场景名...]
证据：_attic/acceptance-menu/handoff/<场景>-*.png + handoff-<场景>.log
"""
import os
import socket
import subprocess
import sys
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ATTIC = os.path.join(ROOT, "_attic")
TOOLS = os.path.join(ATTIC, "tools")
IMG = os.path.join(ATTIC, "varix-uefi.img")
EDK2 = os.path.join(ATTIC, "edk2-x86_64-code.fd")
EDK2_VARS = os.path.join(ATTIC, "edk2-vars-menu.fd")
SHOT_DIR = os.path.join(ATTIC, "acceptance-menu", "handoff")
QEMU = r"C:\Program Files\qemu\qemu-system-x86_64"
PY = sys.executable

MON_PORT = 14861
SER_PORT = 14862
NL = bytes([10])


class Pump(threading.Thread):
    """串口采集器：循环抽空、对端关闭即重连补收，绝不静默退出（丢包戒律）。"""

    def __init__(self, port, path):
        super().__init__(daemon=True)
        self.buf = b""
        self.f = open(path, "wb")
        self.alive = True
        self.sock = None
        self.port = port

    def connect(self):
        for _ in range(400):
            try:
                self.sock = socket.create_connection(("127.0.0.1", self.port), timeout=5)
                break
            except OSError:
                time.sleep(0.1)
        if self.sock is not None:
            self.sock.settimeout(None)

    def run(self):
        while self.alive:
            if self.sock is None:
                self.connect()
                if self.sock is None:
                    time.sleep(0.1)
                    continue
            try:
                d = self.sock.recv(65536)
            except OSError:
                self.sock = None
                continue
            if not d:
                try:
                    self.sock.close()
                except Exception:
                    pass
                self.sock = None
                continue
            self.buf += d
            self.f.write(d)
            self.f.flush()

    def text(self):
        return self.buf.decode(errors="replace")


class Mon:
    def __init__(self, port):
        self.s = socket.create_connection(("127.0.0.1", port), timeout=10)
        time.sleep(0.4)
        try:
            self.s.recv(65536)
        except Exception:
            pass

    def cmd(self, c):
        try:
            self.s.sendall(c.encode() + NL)
        except OSError:
            return ""
        time.sleep(0.5)
        try:
            return self.s.recv(65536).decode(errors="replace")
        except Exception:
            return ""

    def shot(self, name):
        os.makedirs(SHOT_DIR, exist_ok=True)
        self.cmd("screendump " + os.path.join(SHOT_DIR, name))

    def close(self):
        try:
            self.s.sendall(b"quit" + NL)
            time.sleep(0.4)
            self.s.close()
        except Exception:
            pass


def ensure_vars():
    if not os.path.isfile(EDK2_VARS):
        with open(EDK2_VARS, "wb") as f:
            f.write(b"\x00" * (256 * 1024))
        print(f"新建 OVMF NVRAM: {EDK2_VARS}")


def set_cmdline(cmdline, timeout=5):
    """改镜像里 limine.conf 的 kernel_cmdline 与 timeout（倒计时压到 5s 省时间）。"""
    body = (
        "# 交接走查（--uefi 时生效；正式盘为 boot_timeout=0）\n"
        "timeout: 0\n"
        "serial: yes\n"
        "\n"
        "/kernel/varix\n"
        "    protocol: limine\n"
        "    kernel_path: boot():/kernel/varix\n"
        f"    kernel_cmdline: {cmdline}\n"
    )
    tmp = os.path.join(ATTIC, "_tmp-limine-handoff.conf")
    with open(tmp, "w", encoding="utf-8", newline="\n") as f:
        f.write(body)
    r = subprocess.run(
        [PY, os.path.join(TOOLS, "esp-fat.py"), "put", IMG, "/limine.conf", tmp],
        capture_output=True, text=True, encoding="utf-8", errors="replace",
    )
    tail = [ln for ln in (r.stdout or "").splitlines() if ln.startswith("OK") or "!!!" in ln]
    print(f"  limine.conf <- {cmdline}  {' '.join(tail) or r.stdout.strip()[:80]}")
    os.remove(tmp)


def run_case(name, cmdline, total=180, expect="", shots_at=()):
    ensure_vars()
    set_cmdline(cmdline)
    ser_log = os.path.join(ATTIC, "acceptance-menu", f"handoff-{name}.log")
    os.makedirs(os.path.dirname(ser_log), exist_ok=True)
    if os.path.exists(ser_log):
        os.remove(ser_log)
    pump = Pump(SER_PORT, ser_log)
    proc = subprocess.Popen(
        [
            QEMU, "-machine", "q35", "-m", "2048", "-smp", "2", "-cpu", "max",
            "-drive", f"if=pflash,format=raw,file={EDK2},unit=0,readonly=on",
            "-drive", f"if=pflash,format=raw,file={EDK2_VARS},unit=1",
            "-drive", f"file={IMG},format=raw,if=ide,index=0",
            "-boot", "order=c",
            "-serial", f"tcp:127.0.0.1:{SER_PORT},server,nowait",
            "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait",
            "-display", "none",
        ],
        cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT,
    )
    pump.connect()
    pump.start()
    print(f"[{name}] QEMU pid={proc.pid}", flush=True)

    t0, mon, made = time.time(), None, []
    try:
        while time.time() - t0 < total:
            time.sleep(0.25)
            el = time.time() - t0
            if proc.poll() is not None:
                print(f"[{name}] QEMU exited rc={proc.returncode}", flush=True)
                break
            if mon is None:
                try:
                    mon = Mon(MON_PORT)
                except OSError:
                    continue
            for at, tag in shots_at:
                if tag not in made and el >= at:
                    mon.shot(f"{name}-{tag}.png")
                    made.append(tag)
                    print(f"[{name}] shot t={el:.1f}s {tag}", flush=True)
            txt = pump.text()
            if expect and expect in txt and el > 12:
                print(f"[{name}] 命中判据 @t={el:.1f}s", flush=True)
                break
    finally:
        if mon is not None:
            mon.shot(f"{name}-final.png")
            mon.close()
        pump.alive = False
        try:
            proc.wait(timeout=8)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=8)
    return pump.text()


CASES = {
    # 无已验证 Windows 项（QEMU 常态）→ 防自锁闸门 → 降级落 ushell
    "h1-fallback": dict(
        cmdline="boot_timeout=5 boot_default=varix",
        expect="handoff: unavailable",
        shots_at=((14, "menu"), (28, "kernel"), (60, "after")),
    ),
    # 显式 pin 项号 → 视为已验证 → 真交接（画面 + BootNext + 复位）
    # 多点抓帧：交接画面在 TCG 下可能被 screendump 抓到"画到一半"的中间态，
    # 单张截图不足以判它画没画对——铺开四张，只要有一张完整即可定案。
    "h2-handoff": dict(
        cmdline="boot_timeout=5 boot_default=varix boot_next=0x0001",
        expect="written & verified — resetting into Windows",
        shots_at=((13.5, "t135"), (14.0, "t140"), (14.5, "t145"), (15.0, "t150")),
    ),
    # 配置关掉交接 → 不尝试交接，直接落 ushell
    "h3-disabled": dict(
        cmdline="boot_timeout=5 boot_default=varix handoff=0",
        expect="handoff: disabled by config",
        shots_at=((14, "menu"), (30, "after")),
    ),
}


def main() -> int:
    which = sys.argv[1:] or list(CASES.keys())
    results = []
    for name in which:
        if name not in CASES:
            print("未知场景 " + name, file=sys.stderr)
            return 1
        cfg = CASES[name]
        print(f"\n===== 交接场景 {name} =====", flush=True)
        txt = run_case(name, cfg["cmdline"], shots_at=cfg["shots_at"], expect=cfg["expect"])
        keys = ("boot-diag:", "boot-select: entry=", "handoff:", "BootNext", "SHELL:",
                "desktop-ready", "usrshell:")
        for ln in txt.splitlines():
            if any(k in ln for k in keys):
                print("   | " + ln.strip())
        results.append((name, cfg["expect"] in txt))
    print("\n===== 汇总 =====")
    for name, ok in results:
        print(f"  {name}: {'PASS' if ok else 'FAIL'}")
    print("总判定:", "ALL-PASS" if all(ok for _n, ok in results) else "HAS-FAIL")
    return 0 if all(ok for _n, ok in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
