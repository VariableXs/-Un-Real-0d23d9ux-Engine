#!/usr/bin/env python3
r"""x2APIC 复现/验证测试：-smp 2 -cpu max + kernel_cmdline varix.x2apic 强制 x2APIC 模式，
复现实机 Y7000（16 核 + BIOS x2APIC 默认开）的 smp 启动路径。

红测（修复前）：预期 lapic: mode=X2Apic 后卡死，无 smp online（实机现象复现）。
绿测（修复后）：预期 smp: 2 core(s) online + boot complete。

用法: python qemu-x2apic-test.py
前置: cargo kbuild 已产出修复后内核；uefi-esp.vhd 存在。
"""
import ctypes
import os
import subprocess
import sys
import time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
ATTIC = ROOT + r"\_attic"
DISK = ATTIC + r"\uefi-esp.vhd"
KERNEL = ROOT + r"\kernel\target\x86_64-unknown-none\release\varix"
SERIAL = ATTIC + r"\x2apic-test-serial.log"
ERRLOG = ATTIC + r"\x2apic-test-stderr.log"
UPD_SCRIPT = ATTIC + r"\x2apic-vhd-update.ps1"
UPD_LOG = ATTIC + r"\x2apic-vhd-update.log"
CHECK = ROOT + r"\_attic\bcd-fix2-parse-check.py"
MON_PORT = 14463
QEMU = r"C:\Program Files\qemu\qemu-system-x86_64"
EDK2 = ATTIC + r"\edk2-x86_64-code.fd"

TEST_CONF = (
    "# x2APIC repro test config (generated)\n"
    "timeout: 0\n"
    "serial: yes\n"
    "\n"
    "/kernel/varix\n"
    "    protocol: limine\n"
    "    kernel_path: boot():/kernel/varix\n"
    "    kernel_cmdline: varix.x2apic\n"
)


def ps_run(cmd: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["powershell", "-NoProfile", "-Command", cmd], capture_output=True, timeout=120
    )


def update_vhd() -> None:
    """提权挂载 VHD 并写入测试 conf + 新内核（挂 VHD 需要特权 0x80070522）。"""
    for f in (SERIAL, ERRLOG, UPD_LOG):
        if os.path.exists(f):
            os.remove(f)
    with open(ATTIC + r"\x2apic-limine.conf", "w", encoding="utf-8", newline="\n") as f:
        f.write(TEST_CONF)
    # ps1 补 BOM + 语法预检 + ShellExecuteW 提权 + 轮询（bcd-fix 范式）
    raw = open(UPD_SCRIPT, "rb").read()
    if not raw.startswith(b"\xef\xbb\xbf"):
        open(UPD_SCRIPT, "wb").write(b"\xef\xbb\xbf" + raw)
    r = subprocess.run([sys.executable, CHECK, UPD_SCRIPT], capture_output=True)
    out = r.stdout.decode("gbk", "replace") + r.stderr.decode("gbk", "replace")
    if "ParseErrors: 0" not in out:
        print(out)
        raise SystemExit("!! ps1 语法预检未过")
    params = (
        '-NoProfile -ExecutionPolicy Bypass -Command '
        f'try {{ & \'{UPD_SCRIPT}\' *> \'{UPD_LOG}\' }} '
        f'catch {{ $_ | Out-String | Add-Content \'{UPD_LOG}\' }}'
    )
    rc = ctypes.windll.shell32.ShellExecuteW(
        None, "runas", "powershell.exe", params, None, 0)
    if rc <= 32:
        raise SystemExit(f"!! UAC 被取消 (rc={rc})")
    deadline = time.time() + 90
    log = ""
    while time.time() < deadline:
        time.sleep(2)
        if os.path.exists(UPD_LOG):
            log = open(UPD_LOG, "rb").read().decode("utf-16", "replace")
            if "VHD-UPDATE-DONE" in log or "UPDATE-FAIL" in log:
                break
    print(log)
    if "VHD-UPDATE-DONE" not in log:
        raise SystemExit("!! VHD 更新未完成")


def run_qemu() -> str:
    """跑 QEMU，轮询串口，返回判定。"""
    proc = subprocess.Popen(
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
    print(f"[test] QEMU pid={proc.pid} (-smp 2 -cpu max)")
    seen = ""
    verdict = ""
    deadline = time.time() + 240
    while time.time() < deadline:
        time.sleep(3)
        if proc.poll() is not None:
            print(f"[test] QEMU exited rc={proc.returncode}")
            break
        if os.path.exists(SERIAL):
            tail = open(SERIAL, "r", errors="replace").read()
            if tail != seen:
                print(tail[len(seen):], end="", flush=True)
                seen = tail
            if "boot complete" in tail:
                verdict = "FULL-BOOT"
                break
            if "mode=X2Apic" in tail:
                verdict = "STUCK-AFTER-X2APIC"  # 红测特征，等满看有没有后续
    try:
        import socket
        s = socket.create_connection(("127.0.0.1", MON_PORT), timeout=3)
        s.sendall(b"quit\n")
        s.close()
    except OSError:
        pass
    try:
        proc.wait(timeout=8)
    except subprocess.TimeoutExpired:
        proc.kill()
    # 终判
    tail = open(SERIAL, "r", errors="replace").read() if os.path.exists(SERIAL) else ""
    if "boot complete" in tail:
        return "FULL-BOOT (含 mode=%s)" % ("X2Apic" if "X2Apic" in tail else "?")
    if "mode=X2Apic" in tail and "smp:" not in tail:
        return "STUCK-AFTER-X2APIC —— 复现实机卡死特征"
    if "mode=X2Apic" in tail:
        return "X2APIC-RAN-BUT-INCOMPLETE (smp 线见日志)"
    return "NO-X2APIC (检查 -cpu max 是否支持 x2apic)"


if __name__ == "__main__":
    if not os.path.exists(DISK):
        raise SystemExit("!! 先跑 uefi-vhd-build-elevated.py 生成 uefi-esp.vhd")
    if not os.path.exists(KERNEL):
        raise SystemExit("!! 先 cargo kbuild")
    update_vhd()
    result = run_qemu()
    print("\n[test] verdict:", result)
