#!/usr/bin/env python3
"""Variable 系统全页走查冒烟（2026-09-19 第 3 轮）——「Variable 是主系统，
零选项零按键自动进入；菜单五项；重启切 Windows；关机断电」的端到端证明。

exFAT 根目录实测条目序（截图目检）：Games(DIR)/PortableApps(DIR)/big(DIR)/
contig(DIR)/apps.json(322)/README.txt(478) —— 脚本按计数逐项尝试打开。

验证链（QEMU，纯键盘注入）：
  A. Boot → 零按键自动进入（本阶段无任何 sendkey 调用）：
     disp → entering variable-system → first-frame → boot-replay done →
     desktop-ready（加载动画里程碑平滑推进，≥2s 落桌面）；
  B. START 菜单五项逐页走查（菜单 0 起：Files=0/Settings=1/About=2/
     Restart=3/Shutdown=4）：
     - Files：打开 → fm count>0 → 逐项尝试 Enter 直到文件视图打开 → Esc 返回；
     - Settings：打开 → LEFT 调 BOOT TIMEOUT → kv set rc=0（KV 持久通路）；
     - About：打开（VARIABLE SYSTEM 面板，含 shutdown 行）；
     - Restart：高亮确认存在但不按（Esc 返回 = 不误触发）；
     - Shutdown：高亮确认存在但不按（Esc 返回 + 断言零 shutdown 标记）；
  C. 终局·真重启：START → Restart → Enter → SYS_REBOOT → 整机真重启
     （第二次 boot 链）→ 第二次**零按键**自动进入（desktop-ready 基线 +1）；
  D. 终局·真关机：START → Shutdown → Enter → SYS_POWEROFF →
     BIOS 引导无 RS → ACPI S5（PM1a_CNT <- SLP_TYP|SLP_EN）→
     **QEMU 进程退出**（S5 生效的硬判据）。
  全部标记按出现次数递增判定（防历史记录误判），每页 screendump 留证。

串口读取戒律：QEMU `-serial file:` 在 Windows 上低输出量时落盘滞后可达
分钟级（首轮走查 6 个标记误报超时的根因）——实时判定必须走 `-serial
tcp:` socket 实时流，证据文件由脚本 tee 落盘。**连接后必须
settimeout(None)**（socket 超时传染戒律：create_connection 的 timeout 会被
recv 继承，泵线程在首个 >5s 静默窗口（桌面稳态）socket.timeout 退出、
缓冲冻结——次轮走查 600s 全空的根因）。二次引导判定用基线计数（socket
连接前的早期串口行会丢失，不能用绝对次数 2）。

用法：python _attic/qemu-shell-walkthrough.py
证据：_attic/qemu-walk-serial.log（tee）+ _attic/acceptance-shell/*.png
"""
import os
import re
import socket
import subprocess
import sys
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ATTIC = os.path.join(ROOT, "_attic")
ISO = os.path.join(ROOT, "varix-qemu.iso")
TEST_IMG = os.path.join(ATTIC, "walk-testdisk.img")
SHARED_IMG = os.path.join(ATTIC, "kbd-shared-exfat.img")
SERIAL = os.path.join(ATTIC, "qemu-walk-serial.log")
SHOT_DIR = os.path.join(ATTIC, "acceptance-shell")
MON_PORT = 14734
SERIAL_PORT = 14735

BOOT_TIMEOUT = 600
# TCG 戒律（2026-09-19 第 3 轮走查）：键注入 → inputsvc 探针 → ushell 处理
# → marker 落串口的端到端延迟波动可达 60~120s（TCG 时间膨胀，随宿主负载
# 漂移）——窗口给足 180s；逐项尝试 60s/次。窗口不足会连锁污染后续 wait
# 的基线计数（迟到标记被计入基线 → +1 判定永远等不到）。
STEP_TIMEOUT = 180
TRY_WINDOW = 60


def sh(cmd):
    print("+", " ".join(cmd))
    return subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)


def make_disks():
    with open(TEST_IMG, "wb") as f:
        f.truncate(1_048_576 * 512)
        f.seek(60_000 * 512)
        f.write(b"WALK-PROBE".ljust(512, b"0"))
    r = sh([sys.executable, os.path.join(ATTIC, "mkexfat.py"), SHARED_IMG])
    if r.returncode != 0:
        print(r.stdout, r.stderr)
        raise SystemExit("mkexfat failed")
    os.makedirs(SHOT_DIR, exist_ok=True)


class SerialTee:
    """`-serial tcp:` 实时流 → 内存缓冲（实时判定）+ 证据文件（tee 落盘）。"""

    def __init__(self, port, path):
        self.lock = threading.Lock()
        self.buf = b""
        self.alive = True
        self.f = open(path, "wb")
        self.sock = self._connect(port)
        # 超时传染戒律：create_connection 的 timeout 会被 recv 继承——
        # 泵线程会在首个静默窗口（桌面稳态 >5s 无输出）socket.timeout
        # 退出，缓冲冻结。连接成功后必须解除超时。
        self.sock.settimeout(None)
        self.t = threading.Thread(target=self._pump, daemon=True)
        self.t.start()

    def _connect(self, port):
        last = None
        for _ in range(200):
            try:
                return socket.create_connection(("127.0.0.1", port), timeout=5)
            except OSError as e:
                last = e
                time.sleep(0.1)
        raise SystemExit("serial connect failed: " + str(last))

    def _pump(self):
        while self.alive:
            try:
                data = self.sock.recv(65536)
            except OSError:
                break
            if not data:
                break
            with self.lock:
                self.buf += data
            self.f.write(data)
            self.f.flush()

    def count(self, marker):
        with self.lock:
            return self.buf.count(marker.encode())

    def text(self):
        with self.lock:
            return self.buf.decode(errors="replace")

    def close(self):
        self.alive = False
        try:
            self.sock.close()
        except Exception:
            pass
        self.f.close()


class Mon:
    def __init__(self, port):
        self.s = socket.create_connection(("127.0.0.1", port), timeout=10)
        time.sleep(0.5)
        self.s.recv(65536)

    def cmd(self, c):
        try:
            self.s.sendall(c.encode() + b"\n")
        except OSError:
            return ""  # QEMU 已退（S5 关机后 monitor 断连）——静默
        time.sleep(0.6)
        try:
            return self.s.recv(65536).decode(errors="replace")
        except Exception:
            return ""

    def key(self, k):
        self.cmd("sendkey " + k)

    def shot(self, name):
        self.cmd("screendump " + os.path.join(SHOT_DIR, name))


SER = None


def read_log():
    return SER.text()


def count_marker(marker):
    return SER.count(marker)


def wait_count(marker, before, timeout=STEP_TIMEOUT):
    """等标记出现次数从 before 增至 before+1（实时 socket 流，无落盘滞后）。"""
    t0 = time.time()
    while time.time() - t0 < timeout:
        if count_marker(marker) >= before + 1:
            print(f"  [{time.time()-t0:6.1f}s] marker(+1): {marker}")
            return True
        time.sleep(0.05)
    with SER.lock:
        nbytes = len(SER.buf)
    print(f"  [{time.time()-t0:6.1f}s] MARKER TIMEOUT: {marker} (serial buf {nbytes}B)")
    return False


def mark(marker):
    """取当前计数（作为 +1 判定的基线）。"""
    return count_marker(marker)


# wait 超时的迟到候选 (check_name, marker, before)——TCG 键延迟可超窗口，
# 汇总前统一复查计数增长：标记迟到到达即「功能发生」，判 PASS(late)。
PENDING = []


def wait_step(name, marker, before, timeout=STEP_TIMEOUT):
    """+1 判定；超时不判死，记入 PENDING 交给汇总前收割。"""
    ok = wait_count(marker, before, timeout)
    if not ok:
        PENDING.append((name, marker, before))
    return ok


def open_startmenu(mon):
    """桌面 → START 菜单（Esc 先归位），标记计数 +1 判定。"""
    mon.key("esc")
    time.sleep(0.4)
    mon.key("esc")
    time.sleep(0.4)
    before = mark("SHELL: startmenu opened")
    mon.key("spc")
    ok = wait_step("startmenu opened", "SHELL: startmenu opened", before)
    time.sleep(0.4)
    return ok


def menu_open_item(mon, idx, marker, name=None):
    """打开 START 菜单 → down×idx → Enter → 等页面标记 +1。

    startmenu 标记迟到不中断流程（键照发，判定交给收割）——第三跑实测
    startmenu 超时会让后续 down/ent 全部跳过，C/D 阶段假阴性放大。
    """
    open_startmenu(mon)
    for _ in range(idx):
        mon.key("down")
        time.sleep(0.3)
    before = mark(marker)  # 基线必须在 Enter 之前取——发键后取会把快速响应计入基线，+1 判定追空
    mon.key("ret")
    return wait_step(name or marker, marker, before)


def main():
    global SER
    make_disks()
    if not os.path.exists(ISO):
        raise SystemExit("missing ISO: " + ISO + " (先跑 python scripts/make-iso-qemu.py)")
    # 串口端口占用预检（陈旧 QEMU 残留会让新实例 bind 失败直接退出）
    for _ in range(10):
        try:
            probe = socket.create_connection(("127.0.0.1", SERIAL_PORT), timeout=0.3)
            probe.close()
            print(f"  port {SERIAL_PORT} busy — waiting for stale owner to exit...")
            time.sleep(2.0)
        except OSError:
            break
    if os.path.exists(SERIAL):
        os.remove(SERIAL)
    proc = subprocess.Popen(
        [
            "qemu-system-x86_64",
            "-machine", "q35",
            "-cdrom", ISO,
            "-drive", f"file={TEST_IMG},if=none,id=nv1,format=raw",
            "-device", "nvme,drive=nv1,serial=WALKA",
            "-drive", f"file={SHARED_IMG},if=none,id=nv2,format=raw",
            "-device", "nvme,drive=nv2,serial=WALKB",
            "-serial", f"tcp:127.0.0.1:{SERIAL_PORT},server,nowait",
            "-monitor", f"tcp:127.0.0.1:{MON_PORT},server,nowait",
            "-m", "1024",
        ],
        cwd=ROOT,
    )
    SER = SerialTee(SERIAL_PORT, SERIAL)
    checks = []
    try:
        mon = Mon(MON_PORT)

        # A. 零按键自动进入（本阶段与二次进入阶段零 sendkey 调用——
        #    「进界面不给任何选项」的走查级证据）。
        ok = wait_count("SHELL: entering variable-system", 0, BOOT_TIMEOUT)
        checks.append(("zero-input: entering marker appears on its own", ok))
        time.sleep(0.6)
        mon.shot("01-loading.png")
        ok = wait_count("SHELL: boot-replay done", 0, 60)
        checks.append(("zero-input: boot-replay done", ok))
        ok = wait_count("SHELL: desktop-ready", 0, 60)
        checks.append(("zero-input: desktop-ready (auto, no keys sent)", ok))
        time.sleep(0.8)
        mon.shot("02-desktop.png")
        desktop_base = count_marker("SHELL: desktop-ready")
        # 二次进入基线必须在 A 阶段 wait 完成后取（此时首次 entering 已计 1）；
        # 若预取在 wait 前（=0），+1 判定会被 A 阶段那一次立即满足 → 假 PASS。
        entering_base = count_marker("SHELL: entering variable-system")

        # B1. Files 页 + 文件视图（目录条目 Enter=无操作是正确行为，逐项下移）
        checks.append(("startmenu opened", open_startmenu(mon)))
        mon.shot("03-startmenu.png")
        before_files = mark("SHELL: files opened")  # 基线先于发键取（上一版把 mark 放在 ret 后 → 首响计入基线追空）
        before_fm = mark("SHELL: fm count=")
        mon.key("ret")  # 第一项 Files
        ok = wait_step("files opened", "SHELL: files opened", before_files)
        checks.append(("files opened", ok))
        ok = wait_step("fm count reported", "SHELL: fm count=", before_fm)
        checks.append(("fm count reported", ok))
        count = 0
        m = re.findall(r"SHELL: fm count=(\d+)", read_log())
        if m:
            count = int(m[-1])
        print(f"  fm count = {count}")
        checks.append(("files non-empty (count>0)", count > 0))
        time.sleep(0.6)
        mon.shot("04-files.png")
        # 逐项尝试打开（目录 Enter 无操作；文件 Enter → file view）
        opened = False
        for attempt in range(max(count, 1)):
            before = mark("SHELL: file opened name=")
            mon.key("ret")
            if wait_count("SHELL: file opened name=", before, TRY_WINDOW):
                opened = True
                break
            mon.key("down")
            time.sleep(0.4)
        if not opened:
            PENDING.append(("file view opened (file opened name=)",
                            "SHELL: file opened name=", before))
        checks.append(("file view opened (file opened name=)", opened))
        time.sleep(0.6)
        mon.shot("05-fileview.png")
        mon.key("esc")  # fileview → files
        time.sleep(0.5)
        mon.key("esc")  # files → desktop
        time.sleep(0.5)

        # B2. Settings 页（菜单项 1）+ KV 持久通路
        checks.append(("settings opened",
                       menu_open_item(mon, 1, "SHELL: settings opened", name="settings opened")))
        time.sleep(0.5)
        mon.shot("06-settings.png")
        before = mark("SHELL: settings set key=boot_timeout")
        mon.key("left")  # BOOT TIMEOUT -1 → kv_set
        ok = wait_step("kv set boot_timeout rc=0 (KV path alive)",
                       "SHELL: settings set key=boot_timeout", before)
        checks.append(("kv set boot_timeout rc=0 (KV path alive)", ok))
        mon.key("esc")
        time.sleep(0.5)

        # B3. About 页（菜单项 2，VARIABLE SYSTEM 面板含 shutdown 行）
        checks.append(("about opened",
                       menu_open_item(mon, 2, "SHELL: about opened", name="about opened")))
        time.sleep(0.5)
        mon.shot("07-about.png")
        mon.key("esc")
        time.sleep(0.5)

        # B4. Restart 项存在性（菜单项 3：高亮但不按 → Esc = 不误触发）
        if open_startmenu(mon):
            for _ in range(3):
                mon.key("down")
                time.sleep(0.3)
            mon.shot("08-restart-highlight.png")
        mon.key("esc")
        time.sleep(0.5)
        checks.append(("restart NOT triggered on Esc (menu intact)",
                       count_marker("SHELL: restart requested") == 0))

        # B5. Shutdown 项存在性（菜单项 4：高亮但不按 → Esc + 零标记断言）
        if open_startmenu(mon):
            for _ in range(4):
                mon.key("down")
                time.sleep(0.3)
            mon.shot("09-shutdown-highlight.png")
        mon.key("esc")
        time.sleep(0.5)
        checks.append(("shutdown NOT triggered on Esc (menu intact)",
                       count_marker("SHELL: shutdown requested") == 0))

        # C. 终局·真重启（二次引导判定用基线计数，防连接前早期行丢失）
        kbd_base = count_marker("kbd-init: probe=")
        before_8042 = mark("reboot: 8042 pulse")  # 基线在发键前取
        checks.append(("restart requested (SYS_REBOOT dispatched)",
                       menu_open_item(mon, 3, "SHELL: restart requested",
                                      name="restart requested (SYS_REBOOT dispatched)")))
        ok2 = wait_step("kernel reboot ladder entered",
                        "reboot: 8042 pulse", before_8042)
        checks.append(("kernel reboot ladder entered", ok2))
        ok3 = False
        t0 = time.time()
        while time.time() - t0 < 120:
            if count_marker("kbd-init: probe=") >= kbd_base + 1:
                ok3 = True
                break
            time.sleep(0.2)
        checks.append(("machine reset delivered (second boot in serial)", ok3))
        # 第二次零按键自动进入（基线在 A 阶段预取，期间零 sendkey）
        ok4 = wait_count("SHELL: entering variable-system", entering_base, 180)
        checks.append(("second boot: zero-input auto-enter (entering +1)", ok4))
        ok5 = wait_count("SHELL: desktop-ready", desktop_base, 180)
        checks.append(("second boot: desktop-ready auto (+1, no keys sent)", ok5))
        time.sleep(1.5)
        mon.shot("10-second-desktop.png")

        # D. 终局·真关机：菜单项 4 → SYS_POWEROFF → ACPI S5 → QEMU 退出
        before_pow = mark("poweroff: SYS_POWEROFF")      # 基线在发键前取
        before_acpi = mark("poweroff: acpi s5")
        checks.append(("shutdown requested (SYS_POWEROFF dispatched)",
                       menu_open_item(mon, 4, "SHELL: shutdown requested",
                                      name="shutdown requested (SYS_POWEROFF dispatched)")))
        ok6 = wait_step("kernel poweroff path entered",
                        "poweroff: SYS_POWEROFF", before_pow)
        checks.append(("kernel poweroff path entered", ok6))
        ok7 = wait_step("BIOS boot -> ACPI S5 ladder",
                        "poweroff: acpi s5", before_acpi, 30)
        checks.append(("BIOS boot -> ACPI S5 ladder", ok7))
        ok8 = False
        t0 = time.time()
        while time.time() - t0 < 60:
            if proc.poll() is not None:
                ok8 = True
                break
            time.sleep(0.2)
        print(f"  [{time.time()-t0:6.1f}s] qemu exit rc={proc.returncode}")
        checks.append(("S5 delivered: QEMU process exited (power cut)", ok8))
        # 关机后 QEMU 已退出、monitor 断连——不再 screendump（避免死操作）。

        # 迟到收割：wait 窗口内未到的标记，此刻（后续操作已推进数分钟）
        # 复查计数——TCG 迟到标记大多已到达；≥before+1 即功能发生过。
        if PENDING:
            print("\n=== LATE HARVEST (markers that outlived their window) ===")
            for name, marker, before in PENDING:
                hit = count_marker(marker) >= before + 1
                for i, (n, ok) in enumerate(checks):
                    if n == name and not ok:
                        checks[i] = (n, hit)
                print(f"  [late] {name}: {'PASS (marker arrived late)' if hit else 'FAIL (never arrived)'}")

        # 汇总
        log = read_log()
        print("\n=== SHELL WALKTHROUGH CHECKS ===")
        all_ok = True
        for name, ok in checks:
            print(f"  {name}: {'PASS' if ok else 'FAIL'}")
            all_ok = all_ok and ok
        for m in ["SHELL: entering variable-system", "SHELL: desktop-ready",
                  "SHELL: startmenu opened", "SHELL: files opened",
                  "SHELL: file opened name=", "SHELL: settings opened",
                  "SHELL: settings set key=boot_timeout rc=0",
                  "SHELL: about opened", "SHELL: restart requested",
                  "SHELL: shutdown requested", "reboot: SYS_REBOOT",
                  "poweroff: SYS_POWEROFF", "poweroff: acpi s5"]:
            hit = m in log
            print(f"  serial[{m}]: {'PASS' if hit else 'FAIL'}")
            all_ok = all_ok and hit
        bad = [b for b in ("kernel panic", "triple fault", "shutdown failed",
                           "restart failed") if b in log]
        print("  bad-markers:", bad or "none")
        all_ok = all_ok and not bad
        print("\nSHELL WALKTHROUGH VERDICT:", "PASS" if all_ok else "FAIL")
        return 0 if all_ok else 1
    finally:
        SER.close()
        try:
            proc.kill()
        except Exception:
            pass


if __name__ == "__main__":
    raise SystemExit(main())
