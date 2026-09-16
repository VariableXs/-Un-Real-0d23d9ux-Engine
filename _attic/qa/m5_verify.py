# -*- coding: utf-8 -*-
"""M5 实机验证 v4（v3 实机跑分后的记事本会话恢复隔离版）：
v3→v4 修正点：
  5. v3 实机教训：Win11 记事本会话恢复把早前遗留标签（s6-tail.txt）带回新窗
     → WM_CLOSE 弹应用内保存框 → 窗不销毁 → 后端零事件 → C 段超时假失败。
     v4 在 kill 后改名隔离 TabState/WindowState（finally 恢复），保证测试窗
     全是干净 Untitled；post_close 记录返回值与窗口存活复核。
  1-4.（承 v3）before 稳态化；exited/final 相对基准；日志按天自动定位；
     restore 广播几何与收编摆位对比（7px 漂移修复证据）。

A. 最大化/还原同步（embed://native-max 新链路）——SW_MAXIMIZE 后广播
   + IsZoomed=true + 几何=全屏；最小化→恢复后窗口保持最大化（EmbedBridge max
   守卫实证）；SW_NORMAL 真还原 → rect≈收编摆位 rect（GDI 语义统一后应精确匹配）。
B. 占位卡生命周期——收编后 desktop DOM 出现 .vwm-tp；WM_CLOSE → exited 日志
   + 占位窗自动关（R7 复验）。
C. 多窗甄别——两个 notepad 同树，关一窗按 exited 处理（不误派 readopt），
   另一窗存活。
前置：Variable 实例运行中（CDP 9223）；脚本自清残留 notepad。
"""
import ctypes, ctypes.wintypes as wt, glob, json, os, re, subprocess, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from m4_a6_probe import CDP
from m4_probe import u32

LOG_DIR = os.path.expandvars(r"%APPDATA%\com.variable.app\logs")
SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, SW_NORMAL = 3, 6, 9, 1
WM_CLOSE = 0x0010


def resolve_log():
    """applog 按天命名（UTC 天数 = ts/86400000）；容 ±1 天防边界。"""
    day = int(time.time() // 86400)
    for d in (day, day - 1, day + 1):
        p = os.path.join(LOG_DIR, "applog-%d.log" % d)
        if os.path.exists(p):
            return p
    # 兜底：取 mtime 最新的 applog
    cands = glob.glob(os.path.join(LOG_DIR, "applog-*.log"))
    if cands:
        return max(cands, key=os.path.getmtime)
    raise FileNotFoundError("no applog-*.log under " + LOG_DIR)


LOG = resolve_log()


def log_size():
    try:
        return os.path.getsize(LOG)
    except OSError:
        return 0


def wait_log(off, pat, timeout=8, nth=1):
    deadline = time.time() + timeout
    while time.time() < deadline:
        with open(LOG, encoding="utf-8", errors="replace") as f:
            f.seek(off)
            txt = f.read()
        hits = [l for l in txt.splitlines() if re.search(pat, l)]
        if len(hits) >= nth:
            return hits[nth - 1]
        time.sleep(0.25)
    raise TimeoutError("log wait timeout: " + pat)


def wait_log_after(t_epoch, pat, timeout=8):
    """等待时间戳 > t_epoch 且匹配 pat 的日志行（动作后置判定，免时序竞争）。"""
    deadline = time.time() + timeout
    pat_line = re.compile(r"^(\d+(?:\.\d+)?) ")
    while time.time() < deadline:
        with open(LOG, encoding="utf-8", errors="replace") as f:
            for line in f:
                m = pat_line.match(line)
                if not m:
                    continue
                try:
                    ts = float(m.group(1))
                except ValueError:
                    continue
                if ts > t_epoch and re.search(pat, line):
                    return line
        time.sleep(0.25)
    raise TimeoutError("log wait timeout (after %.1f): %s" % (t_epoch, pat))


def rect_of(h):
    rc = wt.RECT()
    u32.GetWindowRect(wt.HWND(h), ctypes.byref(rc))
    return [rc.left, rc.top, rc.right, rc.bottom]


def is_zoomed(h):
    return bool(u32.IsZoomed(wt.HWND(h)))


def is_window(h):
    return bool(u32.IsWindow(wt.HWND(h)))


def show(h, cmd):
    u32.ShowWindow(wt.HWND(h), cmd)


def post_close(h):
    u32.PostMessageW(wt.HWND(h), WM_CLOSE, 0, 0)


def tp_count():
    c = CDP(port=9223, url_sub="desktop")
    return c.evaluate(r'document.querySelectorAll(".vwm-tp").length')


def stable_tp(max_wait=30, interval=1.2):
    """轮询直到连续两次读数相同（看门狗自动收编稳态化）。"""
    t0 = time.time()
    prev = None
    while time.time() - t0 < max_wait:
        cur = tp_count()
        if prev is not None and cur == prev:
            return cur, True
        prev = cur
        time.sleep(interval)
    return prev, False


def kill_notepads():
    # 先优雅关（减少 TabState 脏状态残留），再强杀兜底
    for h in find_notepad_hwnds():
        post_close(h)
    time.sleep(2.0)
    subprocess.run(["taskkill", "/F", "/IM", "notepad.exe"], capture_output=True)
    time.sleep(1.0)


NOTEPAD_PKG = os.path.expandvars(
    r"%LOCALAPPDATA%\Packages\Microsoft.WindowsNotepad_8wekyb3d8bbwe\LocalState"
)


def _rename_path(src, dst):
    try:
        if os.path.exists(src) and not os.path.exists(dst):
            os.rename(src, dst)
            return True
    except OSError:
        pass
    return False


def isolate_notepad_session():
    """v3 实机教训：Win11 记事本会话恢复会把早前 QA 遗留标签（s6-tail.txt）
    带回新窗，文档带状态 → WM_CLOSE 弹应用内 XAML 保存框（EnumWindows 不可见）
    → 窗不销毁 → 后端零事件 → C 段超时。改名隔离 TabState/WindowState（可逆，
    finally 恢复），保证测试窗全是干净 Untitled。"""
    r1 = _rename_path(os.path.join(NOTEPAD_PKG, "TabState"),
                      os.path.join(NOTEPAD_PKG, "TabState.m5bak"))
    r2 = _rename_path(os.path.join(NOTEPAD_PKG, "WindowState"),
                      os.path.join(NOTEPAD_PKG, "WindowState.m5bak"))
    return [r1, r2]


def restore_notepad_session():
    _rename_path(os.path.join(NOTEPAD_PKG, "TabState.m5bak"),
                 os.path.join(NOTEPAD_PKG, "TabState"))
    _rename_path(os.path.join(NOTEPAD_PKG, "WindowState.m5bak"),
                 os.path.join(NOTEPAD_PKG, "WindowState"))


def window_title(h):
    n = u32.GetWindowTextLengthW(h)
    b = ctypes.create_unicode_buffer(n + 1)
    u32.GetWindowTextW(h, b, n + 1)
    return b.value


def wait_all_notepad_closed(max_wait=12):
    """等所有 notepad 窗口消失（占位窗自动关）——清场后再判定。"""
    t0 = time.time()
    while time.time() - t0 < max_wait:
        if not find_notepad_hwnds():
            return True
        time.sleep(0.5)
    return False


def find_notepad_hwnds():
    out = []
    EnumProc = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def cb(h, _):
        n = u32.GetWindowTextLengthW(h)
        if n:
            b = ctypes.create_unicode_buffer(n + 1)
            u32.GetWindowTextW(h, b, n + 1)
            if u32.IsWindowVisible(h) and ("Notepad" in b.value or "记事本" in b.value):
                out.append(h)
        return True
    u32.EnumWindows(EnumProc(cb), 0)
    return out


def main():
    rep = {"log": os.path.basename(LOG), "pass": True}
    kill_notepads()
    rep["tabstate_isolated"] = isolate_notepad_session()
    try:
        code = run_checks(rep)
    finally:
        restore_notepad_session()
        kill_notepads()
    print(json.dumps(rep, ensure_ascii=False, indent=1))
    return code


def run_checks(rep):
    off = log_size()
    try:
        before, stable = stable_tp()
    except Exception as e:
        print(json.dumps({"fatal": "CDP desktop unreachable: %s" % e}))
        return 2
    rep["tp_before"] = before
    rep["tp_before_stable"] = stable
    if not stable:
        print("FATAL: tp count never stabilized (watchdog adoption loop?)", file=sys.stderr)
        return 3

    # ---------- A+B-1: 单窗收编 → 最大化 → 最小化恢复 → 还原 → exited ----------
    subprocess.Popen(["notepad.exe"])
    line = wait_log(off, r"attach notepad\.exe: hwnd=(\d+)", timeout=15)
    hwnd = int(re.search(r"hwnd=(\d+)", line).group(1))
    rep["hwnd"] = hwnd
    rep["title_A"] = window_title(hwnd)
    time.sleep(1.5)  # embed_bounds 摆位
    rep["rect_after_adopt"] = rect_of(hwnd)
    rep["tp_after_adopt"] = tp_count()

    show(hwnd, SW_MAXIMIZE)
    line = wait_log_after(time.time() - 0.5, r"原生最大化.*几何 (-?\d+)x(-?\d+) (\d+)×(\d+)")
    g = re.search(r"几何 (-?\d+)x(-?\d+) (\d+)×(\d+)", line)
    rep["max_broadcast"] = list(map(int, g.groups()))
    rep["zoomed_after_max"] = is_zoomed(hwnd)
    time.sleep(0.8)
    rep["rect_maximized"] = rect_of(hwnd)

    # 最小化 → 恢复：EmbedBridge 守卫实证（无守卫会被拽回收编时的旧矩形）
    show(hwnd, SW_MINIMIZE)
    time.sleep(1.5)
    show(hwnd, SW_RESTORE)
    time.sleep(2.5)
    rep["zoomed_after_min_restore"] = is_zoomed(hwnd)
    rep["rect_after_min_restore"] = rect_of(hwnd)

    # 真还原：SW_NORMAL（时间戳后置判定，免时序竞争）
    t_restore = time.time()
    show(hwnd, SW_NORMAL)
    line = wait_log_after(t_restore, r"原生还原")
    rep["restore_broadcast"] = line.strip()[:160]
    rep["zoomed_after_restore"] = is_zoomed(hwnd)
    time.sleep(0.6)
    rep["rect_after_restore"] = rect_of(hwnd)
    # 核心证据：还原后 GDI rect 与收编摆位精确一致（M2 遗留 7px 漂移已修）
    rep["restore_geom_matches_adopt"] = all(
        abs(a - b) <= 2 for a, b in zip(rep["rect_after_restore"], rep["rect_after_adopt"])
    )
    # 广播几何（桌面客户区物理 px x,y,w,h；桌面无边框全屏时客户原点=屏幕原点）
    # 注意 rect_* 是 l,t,r,b —— 必须换算成 xywh 再比（v4 首跑教训：直比格式错报 false）
    g2 = re.search(r"几何 (-?\d+)x(-?\d+) (\d+)×(\d+)", line)
    if g2:
        rb = list(map(int, g2.groups()))
        rep["restore_broadcast_rect"] = rb
        ar = rep["rect_after_adopt"]
        adopt_xywh = [ar[0], ar[1], ar[2] - ar[0], ar[3] - ar[1]]
        rep["restore_broadcast_matches"] = all(
            abs(a - b) <= 2 for a, b in zip(rb, adopt_xywh)
        )

    # B-1: exited → 占位窗自动关
    t_close = time.time()
    rep["post_close_A_ret"] = bool(u32.PostMessageW(wt.HWND(hwnd), WM_CLOSE, 0, 0))
    wait_log_after(t_close, r"state=exited", timeout=12)
    time.sleep(2.0)
    rep["hwnd_closed_A"] = not is_window(hwnd)
    rep["tp_after_exit"] = tp_count()

    # ---------- C: 多窗甄别 ----------
    off5 = log_size()
    subprocess.Popen(["notepad.exe"])
    time.sleep(0.8)
    subprocess.Popen(["notepad.exe"])
    l1 = wait_log(off5, r"attach notepad\.exe: hwnd=(\d+)", nth=1, timeout=15)
    l2 = wait_log(off5, r"attach notepad\.exe: hwnd=(\d+)", nth=2, timeout=18)
    h1 = int(re.search(r"hwnd=(\d+)", l1).group(1))
    h2 = int(re.search(r"hwnd=(\d+)", l2).group(1))
    rep["two_windows"] = [h1, h2]
    rep["titles_C"] = [window_title(h1), window_title(h2)]
    time.sleep(1.5)
    t_mc = time.time()
    rep["post_close_C_ret"] = bool(u32.PostMessageW(wt.HWND(h1), WM_CLOSE, 0, 0))
    line = wait_log_after(t_mc, r"state=(exited|orphaned)", timeout=12)
    rep["multi_close_line"] = line.strip()[:160]
    rep["multi_close_state"] = "orphaned" if "orphaned" in line else "exited"
    time.sleep(1.5)
    rep["h1_closed"] = not is_window(h1)
    rep["sibling_alive"] = is_window(h2)
    # 收尾：关掉剩余窗口 + 等清场
    post_close(h2)
    rep["cleanup_all_closed"] = wait_all_notepad_closed()
    # 占位卡异步关闭（closeVwmWinSafe 走 IPC）可能慢于 1.5s —— 轮询到稳态再判
    tf, tf_clean = None, False
    t0 = time.time()
    while time.time() - t0 < 10:
        try:
            tf = tp_count()
        except Exception:
            tf = "cdp-error"
            break
        if tf == before:
            tf_clean = True
            break
        time.sleep(1.0)
    rep["tp_final"] = tf
    rep["final_clean"] = tf_clean

    checks = {
        "adopted": rep["tp_after_adopt"] == before + 1,
        "max_broadcast_ok": rep["zoomed_after_max"] and rep["max_broadcast"][2] >= 1900 and rep["max_broadcast"][3] >= 1040,
        "guard_holds": rep["zoomed_after_min_restore"] and rep["rect_after_min_restore"][2] >= 1900,
        "restore_broadcast_ok": (not rep["zoomed_after_restore"]) and rep["restore_geom_matches_adopt"],
        "exited_auto_close": rep["tp_after_exit"] == rep["tp_after_adopt"] - 1,
        "multi_window_discriminated": rep["multi_close_state"] in ("exited", "orphaned") and rep["sibling_alive"] and rep["h1_closed"],
        "final_clean": rep["final_clean"] and rep["cleanup_all_closed"],
    }
    rep["checks"] = checks
    rep["pass"] = all(checks.values())
    return 0 if rep["pass"] else 1


if __name__ == "__main__":
    sys.exit(main())
