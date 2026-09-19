#!/usr/bin/env python3
r"""对照实验：去掉 varix.x2apic（xAPIC/MMIO ICR 模式）跑同一 VHD，
判别 IPI 投递问题出在源侧（x2APIC MSR 0x830）还是目标侧（OVMF 驻留态）。

绿测判据：smp: 2 core(s) online（AP 经 xAPIC ICR 真实上线）。
红测特征：smp: apic 1 failed: Timeout（目标侧问题，与 ICR 通道无关）。
"""
import ctypes
import os
import subprocess
import sys
import time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
ATTIC = ROOT + r"\_attic"
DISK = ATTIC + r"\uefi-esp.vhd"
SERIAL = ATTIC + r"\xapic-control-serial.log"
ERRLOG = ATTIC + r"\xapic-control-stderr.log"
UPD_SCRIPT = ATTIC + r"\x2apic-vhd-update.ps1"
UPD_LOG = ATTIC + r"\x2apic-vhd-update.log"
CHECK = ROOT + r"\_attic\bcd-fix2-parse-check.py"
MON_PORT = 14465
QEMU = r"C:\Program Files\qemu\qemu-system-x86_64"
EDK2 = ATTIC + r"\edk2-x86_64-code.fd"

# 对照 conf：无 varix.x2apic → APIC_BASE bit10 自动探测（QEMU/OVMF 留 xAPIC）
TEST_CONF = (
    "# xAPIC control config (generated)\n"
    "timeout: 0\n"
    "serial: yes\n"
    "\n"
    "/kernel/varix\n"
    "    protocol: limine\n"
    "    kernel_path: boot():/kernel/varix\n"
)


def update_vhd() -> None:
    for f in (SERIAL, ERRLOG, UPD_LOG):
        if os.path.exists(f):
            os.remove(f)
    with open(ATTIC + r"\x2apic-limine.conf", "w", encoding="utf-8", newline="\n") as f:
        f.write(TEST_CONF)
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
    print(f"[ctl] QEMU pid={proc.pid} (xAPIC 对照)")
    tail = ""
    verdict = ""
    deadline = time.time() + 240
    while time.time() < deadline:
        time.sleep(3)
        if proc.poll() is not None:
            break
        if os.path.exists(SERIAL):
            tail = open(SERIAL, "r", errors="replace").read()
            if "smp: 2 core(s) online" in tail:
                verdict = "SMP2-ONLINE"
                break
            if "smp:" in tail and "core(s) online" in tail:
                verdict = "SINGLE-ONLY"
                break
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
    if os.path.exists(SERIAL):
        tail = open(SERIAL, "r", errors="replace").read()
    for line in tail.splitlines():
        if "lapic:" in line or "smp:" in line:
            print("  " + line.strip())
    if not verdict:
        verdict = "TIMEOUT-NO-SMP-LINE"
    return verdict


if __name__ == "__main__":
    update_vhd()
    print("[ctl] verdict:", run_qemu())
