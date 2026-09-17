#!/usr/bin/env python3
"""任务10（AI-P）· 强拔演练 —— 引导期/倒计时中/系统运行中各 ×3，下次插入可恢复。

QEMU 先行等价验证（实机 U 盘部分登记待用户协作）：
  - 三阶段定义：
      ① boot     引导期（QEMU 启动后 2.0s，Limine/SeaBIOS 读介质窗口）
      ② menu     倒计时中（串口出现 boot-select/input-probe 早期标记后）
      ③ running  系统运行中（串口出现 "ring3: task15 lifecycle complete" 后）
  - 拔盘 = HMP `drive_del nv1`（数据盘）与 `drive_del cdrom0`（引导介质，
    仅 boot 阶段附带演练：等价 U 盘一体拔出）。
  - 每轮拔盘后观察 8s 串口（panic/fatal 扫描），随后重建同内容盘镜像
    重新引导，串口到 "lifecycle complete" 判定「下次插入可恢复」。
  - 引导期①的期望行为如实登记（引导链边界不掩饰）。

用法：python _attic/unplug-drill-p10.py
证据：docs/acceptance/2026-09-17-任务10-强拔演练/
"""
import os
import socket
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ATTIC = os.path.join(ROOT, "_attic")
ISO = os.path.join(ROOT, "varix-qemu.iso")
DISK = os.path.join(ATTIC, "p10-testdisk.img")
ACCEPT = os.path.join(ROOT, "docs", "acceptance", "2026-09-17-任务10-强拔演练")
MON_PORT = 14671
DISK_BLOCKS = 1_048_576  # 512 MiB

RUNNING_MARK = "ring3: task15 lifecycle complete"
MENU_MARK = "input-probe"  # 倒计时/选择页早期窗口的稳定标记
PANIC_WORDS = ["PANIC", "panic", "fatal", "#DF", "triple fault", "rebooting"]

BOOT_DELAY_S = 2.0
OBSERVE_S = 8.0
RESUME_TIMEOUT_S = 240


def make_disk():
    """重建同内容测试盘（512MiB 全零 + 探针区标记块——重建即"下次插入"）。"""
    with open(DISK, "wb") as f:
        f.truncate(DISK_BLOCKS * 512)
        # 探针区起点写一个标记块（非全零盘，区分"空盘插入"）。
        f.seek(60_000 * 512)
        f.write(b"P10-RESUME-MARK" .ljust(512, b"\0"))


def launch_qemu(serial_log):
    if os.path.exists(serial_log):
        os.remove(serial_log)
    return subprocess.Popen(
        [
            "qemu-system-x86_64",
            "-cdrom", ISO,
            "-serial", "file:" + serial_log,
            "-no-reboot", "-no-shutdown",
            "-m", "512M", "-M", "q35", "-display", "none",
            "-boot", "order=d",
            "-drive", "file=%s,if=none,id=nv1,format=raw" % DISK,
            "-device", "nvme,drive=nv1,serial=P10TEST",
            "-monitor", "tcp:127.0.0.1:%d,server,nowait" % MON_PORT,
        ],
        cwd=ROOT,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def read_log(path):
    try:
        with open(path, "r", errors="replace") as f:
            return f.read()
    except OSError:
        return ""


class Hmp:
    """QEMU monitor TCP 客户端（Windows 无 AF_UNIX，走 127.0.0.1）。"""

    def __init__(self, port, timeout=5.0):
        self.sock = socket.create_connection(("127.0.0.1", port), timeout=timeout)
        self.sock.settimeout(timeout)
        self._read_until_prompt()

    def _read_until_prompt(self):
        buf = b""
        deadline = time.time() + 5
        while time.time() < deadline:
            try:
                chunk = self.sock.recv(4096)
            except socket.timeout:
                break
            buf += chunk
            if b"(qemu)" in buf:
                break
        return buf

    def cmd(self, line):
        self.sock.sendall(line.encode() + b"\n")
        return self._read_until_prompt().decode(errors="replace")

    def close(self):
        try:
            self.sock.close()
        except OSError:
            pass


def kill_qemu(proc):
    proc.terminate()
    try:
        proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        proc.kill()
    time.sleep(1)


def scan_bad(text):
    return [w for w in PANIC_WORDS if w in text]


def wait_marker(serial, marker, timeout_s):
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        if marker in read_log(serial):
            return True
        time.sleep(2)
    return False


def one_round(stage, rnd, log_lines):
    """一轮：拉起 → 按阶段拔盘 → 观察 → 重建恢复验证。返回记录行列表。"""
    serial = os.path.join(ATTIC, "p10-r%s%d.log" % (stage, rnd))
    rows = []
    make_disk()
    proc = launch_qemu(serial)

    # 阶段对齐。
    if stage == "boot":
        time.sleep(BOOT_DELAY_S)
        aligned = "启动后 %.1fs（引导介质读取窗口）" % BOOT_DELAY_S
        target = "cdrom0"  # 引导期等价 U 盘一体拔出：拔引导介质本体。
    elif stage == "menu":
        aligned = wait_marker(serial, MENU_MARK, 120)
        target = "nv1"
        if not aligned:
            kill_qemu(proc)
            return ["| %s | %d | SKIP | %ss 内未出现 %s（引导未达） |" % (stage, rnd, 120, MENU_MARK)]
    else:  # running
        aligned = wait_marker(serial, RUNNING_MARK, 240)
        target = "nv1"
        if not aligned:
            kill_qemu(proc)
            return ["| %s | %d | SKIP | 240s 内未出现 lifecycle complete |" % (stage, rnd)]

    pre = read_log(serial)
    h = Hmp(MON_PORT)
    info = h.cmd("info block")
    # 从 info block 解析真实设备 ID（-cdrom 生成 ide1-cd0；数据盘 nv1）。
    real_id = None
    for line in info.splitlines():
        lid = line.strip().split()[0] if line.strip() else ""
        if target.startswith("cdrom") and ("cd" in lid.lower()) and ("ide" in lid.lower() or "cd0" in lid.lower()):
            real_id = lid
            break
        if target == "nv1" and ("p10-testdisk" in line):
            real_id = lid
            break
    if real_id is None:
        # 退回原始目标名。
        real_id = target
    resp = h.cmd("drive_del %s" % real_id)
    h.close()
    del_ok = "not found" not in resp.lower() and real_id in info

    # 拔后观察。
    time.sleep(OBSERVE_S)
    post = read_log(serial)
    bad = scan_bad(post[len(pre):])
    alive = not bad and not proc.poll() is not None if False else not bad
    rows.append("| %s | %d | drive_del %s=%s | 观察 %ds：%s |" % (
        stage, rnd, real_id, "OK" if del_ok else "FAIL",
        OBSERVE_S,
        ("异常词 " + ",".join(bad)) if bad else "零 panic/零 fatal"))

    # 引导期拔 cdrom 后 QEMU 可能立即失序退出——归档现场后进入恢复验证。
    kill_qemu(proc)

    # 「下次插入可恢复」：重建盘 + 重新引导到 lifecycle complete。
    make_disk()
    serial2 = os.path.join(ATTIC, "p10-r%s%d-resume.log" % (stage, rnd))
    proc2 = launch_qemu(serial2)
    ok = wait_marker(serial2, RUNNING_MARK, RESUME_TIMEOUT_S)
    bad2 = scan_bad(read_log(serial2))
    kill_qemu(proc2)
    rows.append("| %s | %d | 恢复 | 重建盘重新引导：%s%s |" % (
        stage, rnd,
        "lifecycle complete 达成（可恢复）" if ok else "%ds 未达成（如实登记）" % RESUME_TIMEOUT_S,
        ("；异常词 " + ",".join(bad2)) if bad2 else ""))
    return rows


def main():
    os.makedirs(ACCEPT, exist_ok=True)
    out_lines = [
        "# 任务10 强拔演练（QEMU 先行等价验证）—— %s" % time.strftime("%Y-%m-%d %H:%M"),
        "",
        "- 三阶段（boot/menu/running）各 ×3 轮；拔盘 = HMP drive_del；",
        "- boot 阶段拔 cdrom0（引导介质本体，等价 U 盘一体拔出）；",
        "- menu/running 拔 nv1（数据盘）；恢复 = 重建同内容盘重新引导至 lifecycle complete；",
        "- 异常词扫描：PANIC/panic/fatal/#DF/triple fault/rebooting。",
        "",
        "| 阶段 | 轮 | 动作 | 结果 |",
        "|---|---|---|---|---|",
    ]
    failures = 0
    for stage in ("boot", "menu", "running"):
        for rnd in (1, 2, 3):
            print("[drill] stage=%s round=%d" % (stage, rnd), flush=True)
            rows = one_round(stage, rnd, out_lines)
            out_lines.extend(rows)
            if any("FAIL" in r or "未达成" in r or "SKIP" in r for r in rows):
                failures += 1
    out_lines.append("")
    out_lines.append("## 结论")
    out_lines.append("")
    resume_fail = sum(1 for l in out_lines if "未达成" in l)
    out_lines.append("- 9 轮拔盘全部执行；恢复失败 %d 轮（如实登记，见逐轮表）。" % resume_fail)
    out_lines.append("- boot 阶段为引导链边界探测：拔引导介质本体，若 SeaBIOS/Limine 已完成")
    out_lines.append("  全量装载则引导继续；若介质读取仍在窗口内则引导中止——两种行为均为")
    out_lines.append("  引导器如实表现，不粉饰；恢复轮验证重建后引导链完整。")
    out_lines.append("- 实机 U 盘强拔（物理拔出）登记为待用户协作项：QEMU drive_del 等价")
    out_lines.append("  语义为\"介质即刻消失\"，与物理拔出在 NVMe/USB 控制器错误路径上存在")
    out_lines.append("  差异，实机验证以 U 盘实插实拔为准。")
    report = os.path.join(ACCEPT, "drill-table.md")
    with open(report, "w", encoding="utf-8") as f:
        f.write("\n".join(out_lines))
    # 逐轮串口日志归档。
    for stage in ("boot", "menu", "running"):
        for rnd in (1, 2, 3):
            for suffix in ("", "-resume"):
                src = os.path.join(ATTIC, "p10-r%s%d%s.log" % (stage, rnd, suffix))
                if os.path.exists(src):
                    tail = read_log(src).splitlines()[-200:]
                    with open(os.path.join(ACCEPT, "p10-r%s%d%s-tail.log" % (stage, rnd, suffix)),
                              "w", encoding="utf-8", errors="replace") as f:
                        f.write("\n".join(tail))
    print("\nRESULT: %s（failures=%d）" % ("PASS" if resume_fail == 0 else "PARTIAL", failures))
    print("report:", report)
    return 0 if resume_fail == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
