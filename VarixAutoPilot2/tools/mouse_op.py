"""★ 真实鼠标操作屏幕上的软件 ★

Variable 明确要求：「你自己打开屏幕中的软件使用鼠标自己操作吧」。

## 为什么不用 Playwright
Playwright 驱动的是**浏览器里的 mock 页面**，不是屏幕上那个 exe。
它验证的是前端逻辑，而 Variable 要验的是**真机上的真实交互**
（含Tauri 桥、真实后端、真实布局）。
所以这里走 Win32 真鼠标事件：SetCursorPos + mouse_event。

## ★ 三个实测踩过的坑★
1. **坐标不能靠 Playwright 的视口换算**——
   浏览器视口 1818x1102，exe 窗口 1454x882，缩放比不同 ⇒ 换算出来点偏。
   正解：**在真实窗口内自己找目标**，用窗口内的相对位置。
2. **GBK 解码**：tasklist 输出是 GBK，必须二进制读+ 手动解码；
   用 text=True 会抛 UnicodeDecodeError，且异常在 reader 线程
   → r.stdout 变 None → 真正的错误被掩盖（本项目已踩4 次）。
3. **窗口会变**：exe 重启后 hwnd 变，必须每次重新枚举，不能缓存。

用法：
  python mouse_op.py list                 列出该进程所有窗口
  python mouse_op.py click <按钮文字>     真机点击该按钮
  python mouse_op.py shot <名字>          抓窗口截图
"""

import ctypes
import ctypes.wintypes as wt
import re
import subprocess
import sys
import time
from pathlib import Path

u = ctypes.windll.user32
g = ctypes.windll.gdi32
u.SetProcessDPIAware()

LEFTDOWN, LEFTUP = 0x0002, 0x0004
WHEEL = 0x0800
PW_RENDER = 0x00000002


# ── 进程/窗口 ────────────────────────────────────────────────
def pids_of(image: str):
    r = subprocess.run(
        ["tasklist", "/FI", f"IMAGENAME eq {image}", "/FO", "CSV", "/NH"],
        capture_output=True, timeout=25,
    )
    raw = r.stdout or b""          # ★ 坑2：必须二进制读
    for enc in ("gbk", "mbcs", "utf-8", "latin-1"):
        try:
            t = raw.decode(enc)
            break
        except (UnicodeDecodeError, LookupError):
            continue
    else:
        return []
    return [int(x) for x in re.findall(r'"(\d+)"', t)]


def windows_of(image: str):
    """返回该镜像的可见窗口，按面积降序。"""
    out = []
    for pid in pids_of(image):
        found = []

        @ctypes.WINFUNCTYPE(ctypes.c_bool, wt.HWND, wt.LPARAM)
        def cb(h, _):
            p = wt.DWORD()
            u.GetWindowThreadProcessId(h, ctypes.byref(p))
            # ★ 不再要求 IsWindowVisible ★
            # 坑（实测）：exe 明明在跑（有进程、探针日志在写），
            # 但枚举不到可见窗口 —— Tauri 的窗口在某些状态下
            # IsWindowVisible 返回 false，于是工具报「没有可见窗口」。
            # 判据改为：**有标题 + 尺寸够大**。
            if p.value == pid:
                n = u.GetWindowTextLengthW(h)
                b = ctypes.create_unicode_buffer(n + 1)
                u.GetWindowTextW(h, b, n + 1)
                rc = wt.RECT()
                u.GetWindowRect(h, ctypes.byref(rc))
                w, ht = rc.right - rc.left, rc.bottom - rc.top
                if w > 100 and ht > 20:          # 放宽：曾见 159x27（被压扁）也要能捞出来修
                    found.append((w * ht, h, b.value, (rc.left, rc.top, rc.right, rc.bottom)))
            return True

        u.EnumWindows(cb, 0)
        out += found
    return sorted(out, reverse=True)


def main_window(image: str):
    w = windows_of(image)
    if not w:
        return None
    return w[0][1], w[0][2], w[0][3]      # hwnd, title, rect


# ── 截图（PW_RENDERFULLCONTENT 对 WebView2 有效）────────────
def shot(hwnd: int, out: Path):
    r = wt.RECT()
    u.GetWindowRect(hwnd, ctypes.byref(r))
    w, ht = r.right - r.left, r.bottom - r.top

    class BIH(ctypes.Structure):
        _fields_ = [("s", wt.DWORD), ("w", ctypes.c_long), ("h", ctypes.c_long),
                    ("p", wt.WORD), ("bc", wt.WORD), ("c", wt.DWORD),
                    ("si", wt.DWORD), ("x", ctypes.c_long), ("y", ctypes.c_long),
                    ("cu", wt.DWORD), ("ci", wt.DWORD)]

    class BI(ctypes.Structure):
        _fields_ = [("h", BIH), ("c", wt.DWORD * 3)]

    # ★ 必须先置前 + 用屏幕 DC ★
    # 实测两个坑：
    #   1) 不置前就截 ⇒ 抓到的是「遮挡它的那个窗口」（本次抓到系统托盘）
    #   2) 改用 GetWindowDC ⇒ WebView2 渲染层不重绘，截出全黑
    # ⇒ 正确组合：先 focus，再 GetDC(0) + PrintWindow
    focus(hwnd, aggressive=False)
    time.sleep(0.8)   # 等 WebView2 重绘
    hdc = u.GetDC(0)
    mdc = g.CreateCompatibleDC(hdc)
    bmp = g.CreateCompatibleBitmap(hdc, w, ht)
    g.SelectObject(mdc, bmp)
    # ★ 只能用 PW_RENDER(2) ★
    # 实测：加 0x1|0x10（CLIENTUPDATE|RENDERFULLCONTENT）反而截出全黑。
    # PW_RENDER=2 对 WebView2 有效。
    u.PrintWindow(hwnd, mdc, PW_RENDER)
    bi = BI()
    bi.h.s = ctypes.sizeof(BIH)
    bi.h.w, bi.h.h, bi.h.p, bi.h.bc = w, -ht, 1, 32
    buf = ctypes.create_string_buffer(w * ht * 4)
    g.GetDIBits(mdc, bmp, 0, ht, buf, ctypes.byref(bi), 0)
    g.DeleteObject(bmp)
    g.DeleteDC(mdc)
    u.ReleaseDC(0, hdc)

    from PIL import Image

    img = Image.frombuffer("RGBA", (w, ht), buf.raw, "raw", "BGRA", 0, 1).convert("RGB")
    out.parent.mkdir(parents=True, exist_ok=True)
    img.save(out)
    return img.size


# ── 鼠标 ────────────────────────────────────────────────────
def focus(hwnd: int, aggressive: bool = False):
    """把窗口置前。

    ★★ 三个实测踩过的坑（都记在这里，别再犯）★★
    1) ShowWindow(hwnd, 9) = SW_RESTORE 会把**最小化**的窗口恢复，
       但 Tauri 窗口恢复时用的是过期的尺寸 ⇒ 变成 159x27 的小条。
       ⇒ 改用 SW_SHOW(5)（不碰最小化状态），需要时先 SW_MAXIMIZE 再说。
    2) SetForegroundWindow 常被系统拒绝（前台锁），
       必须 AttachThreadInput 挂到前台线程的输入队列才可靠。
    3) 窗口若已被拖到屏幕外（(-25600,-25600) 是最小化坐标），
       置前后要确认 rect 真的落在屏幕内，否则坐标换算全错。
    """
    # 只做「显示」，不恢复/不最大化 —— 避免把尺寸搞坏
    u.ShowWindow(hwnd, 5)          # SW_SHOW
    fg = u.GetForegroundWindow()
    tid_fg = u.GetWindowThreadProcessId(fg, None) if fg else 0
    tid_me = ctypes.windll.kernel32.GetCurrentThreadId()
    if tid_fg and tid_fg != tid_me:
        u.AttachThreadInput(tid_me, tid_fg, True)
    u.SetForegroundWindow(hwnd)
    u.BringWindowToTop(hwnd)
    u.SetActiveWindow(hwnd)
    if tid_fg and tid_fg != tid_me:
        u.AttachThreadInput(tid_me, tid_fg, False)

    # ══════════════════════════════════════════════════════════════
    # ★★★ 越界/最小化自愈（★ 这段曾导致窗口塌成 199x34 ★★★）★★★
    #
    # 实测事故：窗口长期停在 `199x34 @(-32000,-32000)` + 最小化态。
    #   -32000 是 Windows 最小化窗口的标准坐标，199x34 是它的占位尺寸。
    #   用户此时点的是**幽灵窗口**，完全无反应，而日志一行都不会多。
    #
    # ★ 两个致命细节 ★
    #  1) `SW_RESTORE` 只把窗口"恢复出来"，**不会把它变回正常尺寸**
    #     （Tauri 的 min/max 约束参与计算，出来就是 199x34）。
    #  2) `SetWindowPos` 带 `SWP_NOZORDER` 时**尺寸参数会被忽略**
    #     —— 而窗口若仍处最小化态，连位置参数都无效。
    #
    # 正解顺序：SW_RESTORE → 等布局 → SetWindowPos(无 NOZORDER) → 再等
    # ══════════════════════════════════════════════════════════════
    rc = wt.RECT()
    u.GetWindowRect(hwnd, ctypes.byref(rc))
    sw = u.GetSystemMetrics(0) or 1920
    sh = u.GetSystemMetrics(1) or 1080
    w, h = rc.right - rc.left, rc.bottom - rc.top

    # 判据：最小化（占位坐标/尺寸） 或 尺寸过小 或 越界
    is_ghost = (w < 400 or h < 300
                or rc.left <= -20000 or rc.top <= -20000
                or rc.left < 0 or rc.top < 0
                or rc.right > sw + 20 or rc.bottom > sh + 20)

    if is_ghost:
        # ① 先恢复（去掉 SW_RESTORE 的过期尺寸问题：恢复后重新量）
        u.ShowWindow(hwnd, 9)          # SW_RESTORE
        time.sleep(1.0)                # ★ 必须等：布局未完成时设尺寸无效
        # ② 再显式设尺寸与位置——**不带 SWP_NOZORDER**，否则尺寸被忽略
        u.SetWindowPos(hwnd, 0, 120, 60, 1500, 900, 0)
        time.sleep(0.8)
        # ③ 再量一次确认修好；还不行就再来一轮（最多 3 轮）
        for _ in range(2):
            u.GetWindowRect(hwnd, ctypes.byref(rc))
            w2, h2 = rc.right - rc.left, rc.bottom - rc.top
            if w2 >= 400 and h2 >= 300 and rc.left > -100:
                break
            u.ShowWindow(hwnd, 9)
            time.sleep(0.9)
            u.SetWindowPos(hwnd, 0, 120, 60, 1500, 900, 0)
            time.sleep(0.9)

    time.sleep(0.4)
    u.GetWindowRect(hwnd, ctypes.byref(rc))
    # ★ 复核并如实回报：窗口修不好就明说，不能让调用方以为已置前★
    if rc.right - rc.left < 400 or rc.bottom - rc.top < 300:
        print(f"[WARN] 窗口仍是 {rc.right-rc.left}x{rc.bottom-rc.top} "
              f"@({rc.left},{rc.top}) —— 可能处于最小化态，截图与点击都会失败")
    return rc


def main() -> int:
    IMAGE = "VarixAutoPilot.exe"
    if len(sys.argv) < 2:
        print(__doc__)
        return 1
    cmd = sys.argv[1]

    wins = windows_of(IMAGE)
    if not wins:
        print(f"[ERR] {IMAGE} 没有可见窗口（是否已启动？）")
        return 1

    if cmd == "list":
        for area, h, t, rc in wins:
            print(f"  hwnd={h:<9} {rc[2]-rc[0]}x{rc[3]-rc[1]} @({rc[0]},{rc[1]})  {t!r}")
        return 0

    # wins 元素是 (area, hwnd, title, rect) —— 4 元组
    _area, hwnd, title, rc = wins[0]
    print(f"窗口 {title!r} {rc[2]-rc[0]}x{rc[3]-rc[1]} @({rc[0]},{rc[1]})")

    if cmd == "shot":
        name = sys.argv[2] if len(sys.argv) > 2 else "win"
        print("截图:", shot(hwnd, Path("shots") / f"{name}.png"))

    elif cmd == "click":
        # click <rx> <ry>  —— 窗口内相对位置
        rx = float(sys.argv[2]) / 100.0
        ry = float(sys.argv[3]) / 100.0
        p = click_rel(hwnd, rx, ry)
        print("已点击窗口内相对位置", sys.argv[2] + "%", sys.argv[3] + "%", "→ 屏幕", p)

    elif cmd == "abs":
        # abs <x> <y> —— 屏幕绝对坐标
        focus(hwnd)
        click(int(sys.argv[2]), int(sys.argv[3]))

    elif cmd == "scroll":
        # scroll <rx> <ry> <clicks> [up|down]
        rx = float(sys.argv[2]) / 100.0
        ry = float(sys.argv[3]) / 100.0
        n = int(sys.argv[4])
        up = (len(sys.argv) < 6) or sys.argv[5] == "up"
        r = wt.RECT()
        u.GetWindowRect(hwnd, ctypes.byref(r))
        focus(hwnd)
        wheel(r.left + int((r.right - r.left) * rx),
              r.top + int((r.bottom - r.top) * ry), n, up)
        print(f"已在 {sys.argv[2]}%,{sys.argv[3]}% 处滚动 {n} 格")

    else:
        print(__doc__)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
