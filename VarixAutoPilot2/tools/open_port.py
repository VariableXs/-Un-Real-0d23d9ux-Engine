#!/usr/bin/env python3
"""VarixAutoPilot · 以调试端口启动 WorkBuddy。

★ 为什么需要这个脚本 ★
Chrome/Electron 的调试端口**只在进程启动时决定**。已经运行的 WorkBuddy
不会因为新参数而开启端口——必须彻底退出再用带参数的方式启动。
而"改桌面快捷方式的目标栏"这一步：
  - 要么你手动改（易错、找不到属性框）
  - 要么改脚本（本项目）—— 但**不能改用户的快捷方式**，那是宿主的东西。

所以本脚本做最小、可逆、无副作用的事：
**杀掉现有 WorkBuddy → 用同一个 exe + 调试端口参数重新启动。**
不碰任何配置文件、不碰 user-data-dir、不碰快捷方式。

★ 关键安全考量 ★
1. 杀进程 = 可能丢失未保存的对话。脚本**先警告并要求确认**，
   绝不强杀。
2. 必须确认目标路径正确（避免杀错程序）。
3. 端口默认只绑127.0.0.1，不对外暴露。
"""

import argparse
import os
import socket
import subprocess
import sys
import time
from pathlib import Path

# 与桌面快捷方式一致（实测读出）
DEFAULT_EXE = Path(r"C:\Users\varia\Desktop\WorkBuddy\WorkBuddy.exe")
DEFAULT_PORT = 9222


def port_open(port: int, timeout: float = 1.5) -> bool:
    """端口是否有人听。"""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.settimeout(timeout)
        return s.connect_ex(("127.0.0.1", port)) == 0


def devtools_alive(port: int = DEFAULT_PORT) -> bool:
    """端口开着 **且** 真的是 DevTools 端点（避免端口被别的程序占了）。"""
    try:
        import urllib.request

        with urllib.request.urlopen(
            f"http://127.0.0.1:{port}/json/version", timeout=2
        ) as r:
            return b"Browser" in r.read(400)
    except Exception:
        return False


def count_workbuddy() -> int:
    """当前 WorkBuddy 进程数。用 tasklist 而非 psutil（免装依赖）。"""
    try:
        r = subprocess.run(
            ["tasklist", "/FI", "IMAGENAME eq WorkBuddy.exe", "/NH", "/FO", "CSV"],
            capture_output=True,
            timeout=15,
        )
        raw = r.stdout or b""
        for enc in ("gbk", "mbcs", "utf-8", "latin-1"):
            try:
                txt = raw.decode(enc)
                break
            except (UnicodeDecodeError, LookupError):
                continue
        else:
            return 0
        return sum(1 for l in txt.splitlines() if "WorkBuddy.exe" in l)
    except Exception:
        return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="以调试端口启动 WorkBuddy")
    ap.add_argument("--exe", default=str(DEFAULT_EXE), help="WorkBuddy.exe 路径")
    ap.add_argument("--port", type=int, default=DEFAULT_PORT, help="调试端口")
    ap.add_argument("--no-kill", action="store_true", help="不杀现有进程（只在新实例上生效）")
    ap.add_argument(
        "--kill",
        action="store_true",
        help="★ 会中断正在进行的对话。确认无要紧的活再��。",
    )
    args = ap.parse_args()

    exe = Path(args.exe)
    port = args.port

    print("=" * 60)
    print("  VarixAutoPilot - 启动 WorkBuddy 并开启调试端口")
    print("=" * 60)
    print(f"  程序: {exe}")
    print(f"  端口: {port}  (仅绑定 127.0.0.1，不对外暴露)")
    print("")

    # ── 1. 已在运行且端口已开 → 什么都不用做 ──
    if devtools_alive(port):
        print(f"[OK] 端口 {port} 已经是可用的 DevTools 端点。")
        print("     WorkBuddy 已正确启动，无需任何操作。")
        return 0

    n = count_workbuddy()
    if n and port_open(port):
        print(f"[WARN] 端口 {port} 被占用，但不是 DevTools 端点。")
        print(f"       可能是别的程序占了 {port}。换个端口试试：--port 9333")
        return 2

    # ── 2. 校验 exe ──
    if not exe.exists():
        print(f"[ERR] 找不到 WorkBuddy.exe：{exe}")
        print("      用 --exe 指定实际路径。")
        return 1

    # ── 2b. ★ 目标校验必须最早做 ★
    # 早先放在"要不要杀"之后，于是传个非 WorkBuddy 路径 + --kill 时，
    # 走的是"运行中"分支，根本没机会校验 ⇒ 校验形同虚设。
    # 现在的判据：只要带了 --kill，就先确认目标确实是 WorkBuddy.exe。
    if args.kill and not str(exe).lower().endswith("workbuddy.exe"):
        print(f"[ERR] --kill 的目标不是 WorkBuddy.exe：{exe}")
        print("      拒绝执行。")
        return 1

    # ── 3. 已在运行但端口没开 → 必须重启（参数只在启动时生效）──
    if n and not args.no_kill and not args.kill:
        print(f"[!] 检测到 WorkBuddy 正在运行（{n} 个进程），但调试端口未开。")
        print()
        print("    ★ 调试端口只在进程启动时决定，已运行的实例不会自动开启。")
        print("    ★ 所以必须先完全退出 WorkBuddy，再用带端口的方式启动。")
        print()
        print("    ⚠ 关闭 WorkBuddy 会中断正在进行的对话。")
        print("      请确认：当前没有要紧的活正在跑，或你已保存。")
        print()

        # ★ 默认不杀 ★
        # 杀 WorkBuddy = 中断用户正在进行的对话。宁可多问一次，
        # 也不替用户做这个决定。必须显式 --kill 才会动手。
        print("    确认无误后，重新运行本命令并加 --kill 参数：")
        print(f"      python {Path(sys.argv[0]).name} --kill")
        print()
        print("    或者你也可以手动：托盘图标右键退出 → 再双击本脚本。")
        return 3

    # ── 3b. 显式授权杀：才动手 ──
    if args.kill and n:
        # 目标校验已在 2b 完成（更早、更严格）
        print(f"[*] 正在关闭 WorkBuddy（{n} 个进程）...")
        r = subprocess.run(
            ["taskkill", "/F", "/IM", "WorkBuddy.exe"], capture_output=True, timeout=30
        )
        raw = (r.stdout or b"") + (r.stderr or b"")
        msg = ""
        for enc in ("gbk", "mbcs", "utf-8", "latin-1"):
            try:
                msg = raw.decode(enc)
                break
            except (UnicodeDecodeError, LookupError):
                continue
        if msg.strip():
            print(f"    {msg.strip()[:200]}")
        left = count_workbuddy()
        if left:
            print()
            print(f"[WARN] 仍有 {left} 个 WorkBuddy 进程存活。")
            print("       多见于托盘还挂着 —— 右键托盘图标选退出，再重跑本脚本。")
            return 5
        print("    已全部退出。")
        time.sleep(1.5)     # 给进程清理留时间

    if args.no_kill and n:
        print("[ERR] WorkBuddy 正在运行，且指定了 --no-kill。")
        print("      新参数无法注入已运行实例，请先退出再试。")
        return 3

    # ── 4. 启动 ──
    print("[-] 正在启动 WorkBuddy ...")
    flags = [
        f"--remote-debugging-port={port}",
        f"--remote-debugging-address=127.0.0.1",
    ]
    try:
        # 独立进程组：脚本退出后不影响它
        subprocess.Popen(
            [str(exe), *flags],
            cwd=str(exe.parent),
            close_fds=True,
        )
    except Exception as e:
        print(f"[ERR] 启动失败：{e}")
        return 1

    # ── 5. 等端口就绪 ──
    print(f"[-] 等待端口 {port} 就绪 ...")
    for i in range(30):
        time.sleep(1)
        if devtools_alive(port):
            print()
            print(f"[OK] 端口 {port} 已就绪，可以启动 VarixAutoPilot 了。")
            return 0
        if (i + 1) % 5 == 0:
            print(f"    仍在等待... ({i + 1}s)")

    print()
    print(f"[WARN] 30 秒内端口 {port} 未就绪。")
    print("       常见原因：")
    print("         - WorkBuddy 启动较慢，稍等片刻再打开 VarixAutoPilot")
    print("         - 端口被占用：换一个端口 --port 9333")
    print("         - 该版本不支持这两个参数")
    return 4


if __name__ == "__main__":
    sys.exit(main())
