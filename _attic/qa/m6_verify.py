# -*- coding: utf-8 -*-
"""M6 泛化验收（M6-1）：对单个第三方应用执行完整嵌入生命周期，七项判定。
用法：python m6_verify.py <notepad|weixin|qq|edge>
复用 M5 的判定骨架（before 稳态化 / 相对基准 / TabState 隔离仅对 notepad /
GDI 几何对账），按应用参数化启动命令与层级预期。

七项：adopted / max_sync / min_guard / restore_geom / exited_autoclose /
      tier_expect / final_clean
"""
import ctypes, ctypes.wintypes as wt, glob, json, os, re, subprocess, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from m4_a6_probe import CDP
from m4_probe import u32

LOG_DIR = os.path.expandvars(r"%APPDATA%\com.variable.app\logs")
SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, SW_NORMAL = 3, 6, 9, 1
WM_CLOSE = 0x0010

APPS = {
    # name: (launch args, image 名, 预期层级 L1/L3, 会话残留等待秒)
    "notepad": (["notepad.exe"], "notepad.exe", "L1", 2, "exited"),
    "weixin":  (["C:/Program Files/Tencent/Weixin/Weixin.exe"], "weixin.exe", "L1", 3, "exited"),
    "qq":      (["C:/Program Files/Tencent/QQNT/QQ.exe"], "qq.exe", "L3", 3, "exited"),
    "edge":    (["C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
                 "--user-data-dir=" + os.path.expandvars(r"%TEMP%\m6_edge_profile"),
                 "--no-first-run", "--no-default-browser-check",
                 "--new-window", "https://example.com"], "msedge.exe", "L3", 3, "exited|orphaned"),
}


def resolve_log():
    day = int(time.time() // 86400)
    for d in (day, day - 1, day + 1):
        p = os.path.join(LOG_DIR, "applog-%d.log" % d)
        if os.path.exists(p):
            return p
    cands = glob.glob(os.path.join(LOG_DIR, "applog-*.log"))
    return max(cands, key=os.path.getmtime)


LOG = resolve_log()


def log_size():
    try:
        return os.path.getsize(LOG)
    except OSError:
        return 0


def wait_log_after(t_epoch, pat, timeout=15):
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


def is_visible(h):
    return bool(u32.IsWindowVisible(wt.HWND(h)))


def show(h, cmd):
    u32.ShowWindow(wt.HWND(h), cmd)


def post_close(h):
    return bool(u32.PostMessageW(wt.HWND(h), WM_CLOSE, 0, 0))


def window_title(h):
    n = u32.GetWindowTextLengthW(h)
    b = ctypes.create_unicode_buffer(n + 1)
    u32.GetWindowTextW(h, b, n + 1)
    return b.value


def class_name(h):
    b = ctypes.create_unicode_buffer(64)
    u32.GetClassNameW(wt.HWND(h), b, 64)
    return b.value


def tp_count():
    c = CDP(port=9223, url_sub="desktop")
    return c.evaluate(r'document.querySelectorAll(".vwm-tp").length')


def stable_tp(max_wait=30, interval=1.2):
    t0 = time.time()
    prev = None
    while time.time() - t0 < max_wait:
        cur = tp_count()
        if prev is not None and cur == prev:
            return cur, True
        prev = cur
        time.sleep(interval)
    return prev, False


def find_windows_of_image(image):
    """按进程映像名找可见顶层主窗（含 pid 反查，规避类名猜测）。"""
    import ctypes
    out = []
    pid_map = {}
    # pid → image
    EnumProc = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def cb(h, _):
        if u32.IsWindowVisible(wt.HWND(h)):
            n = u32.GetWindowTextLengthW(h)
            if n:
                out.append(h)
        return True
    u32.EnumWindows(EnumProc(cb), 0)
    for h in out:
        pid = wt.DWORD()
        u32.GetWindowThreadProcessId(wt.HWND(h), ctypes.byref(pid))
        pid_map.setdefault(pid.value, []).append(h)
    hits = []
    for line in subprocess.run(["tasklist", "/FO", "CSV", "/NH"], capture_output=True).stdout.decode("gbk", "replace").splitlines():
        parts = [p.strip('"') for p in line.split('","')]
        if len(parts) >= 2 and parts[0].lower() == image.lower():
            try:
                pid = int(parts[1])
            except ValueError:
                continue
            hits.extend(pid_map.get(pid, []))
    return hits


def wait_pid_windows(image, want_title_sub=None, timeout=20):
    """等待该映像出现可见主窗（可选标题过滤），返回 hwnd 列表。"""
    t0 = time.time()
    while time.time() - t0 < timeout:
        hs = [h for h in find_windows_of_image(image) if is_window(h)]
        if hs and (want_title_sub is None or any(want_title_sub in window_title(h) for h in hs)):
            return hs
        time.sleep(0.5)
    return []


def main():
    if len(sys.argv) < 2 or sys.argv[1] not in APPS:
        print("usage: m6_verify.py <notepad|weixin|qq|edge>")
        return 2
    name = sys.argv[1]
    args, image, tier_expect, settle, terminal_expect = APPS[name]
    rep = {"app": name, "log": os.path.basename(LOG)}

    t_epoch = time.time()
    try:
        before, stable = stable_tp()
    except Exception as e:
        print(json.dumps({"fatal": "CDP desktop unreachable: %s" % e}))
        return 2
    rep["tp_before"] = before
    rep["tp_before_stable"] = stable
    if not stable:
        return 3

    # 启动（预清理已运行实例：避免单例复用旧窗导致 attach 日志早于 t_epoch）
    # 韧性：Edge 等应用强杀后偶发拒开窗（崩溃恢复态）→ 30s 未 attach 再补一次启动
    pat = r"attach [^\n]*?%s[^\n]*?→ 层级 (L\d)" % re.escape(image)
    line = None
    for attempt in range(2):
        subprocess.Popen(args)
        try:
            line = wait_log_after(t_epoch - 0.5, pat, timeout=30)
            break
        except TimeoutError:
            if attempt == 0:
                subprocess.run(["taskkill", "/F", "/IM", image], capture_output=True)
                time.sleep(2.0)
                t_epoch = time.time()
                continue
            raise
    t_launch = time.time()
    # attach 判定走日志（后端唯一事实源）
    m = re.search(r"→ 层级 (L\d)", line)
    rep["tier_actual"] = m.group(1) if m else "?"
    rep["attach_line"] = line.strip()[:150]

    hs = wait_pid_windows(image, timeout=15)
    if not hs:
        print(json.dumps({"fatal": "no visible window of " + image}))
        return 4
    # 主窗 = 面积最大者
    hwnd = max(hs, key=lambda h: (lambda r: (r[2] - r[0]) * (r[3] - r[1]))(rect_of(h)))
    rep["hwnd"] = hwnd
    rep["title"] = window_title(hwnd)
    rep["cls"] = class_name(hwnd)
    time.sleep(1.5 + settle)  # embed_bounds 摆位 + 大应用首帧
    rep["rect_after_adopt"] = rect_of(hwnd)
    rep["visible_after_adopt"] = is_visible(hwnd)
    rep["tp_after_adopt"] = tp_count()

    # 最大化同步：广播到达 + IsZoomed=true（同步忠实性语义）。
    # 全屏宽度只是多数应用的属性（微信 4.x 自身最大化就不满屏——裸 Windows
    # 基线复现同款，与 Variable 无关），故满屏仅作记录项。
    show(hwnd, SW_MAXIMIZE)
    line = wait_log_after(time.time() - 0.5, r"原生最大化.*几何 (-?\d+)x(-?\d+) (\d+)×(\d+)", timeout=10)
    g = re.search(r"几何 (-?\d+)x(-?\d+) (\d+)×(\d+)", line)
    rep["max_broadcast"] = list(map(int, g.groups()))
    rep["zoomed_after_max"] = is_zoomed(hwnd)
    time.sleep(1.2)
    rep["rect_maximized"] = rect_of(hwnd)
    rep["fullscreen_achieved"] = rep["rect_maximized"][2] - rep["rect_maximized"][0] >= 1900

    # 最小化 → 恢复守卫：保持最大化 + 几何 ≈ 最大化稳态矩形（不被 embedBounds 拽回）
    show(hwnd, SW_MINIMIZE)
    time.sleep(1.5)
    show(hwnd, SW_RESTORE)
    time.sleep(2.5)
    rep["zoomed_after_min_restore"] = is_zoomed(hwnd)
    rep["rect_after_min_restore"] = rect_of(hwnd)
    # 守卫语义 = 位置不被拽回（x,y 稳定）+ 保持最大化。尺寸漂移是应用自身
    # 行为（微信 bare 基线也漂），不纳入判定。
    rep["min_guard_holds"] = (
        abs(rep["rect_after_min_restore"][0] - rep["rect_maximized"][0]) <= 2
        and abs(rep["rect_after_min_restore"][1] - rep["rect_maximized"][1]) <= 2
    )

    # 还原几何对账（GDI 语义）
    t_restore = time.time()
    show(hwnd, SW_NORMAL)
    line = wait_log_after(t_restore, r"原生还原", timeout=10)
    time.sleep(0.6)
    rep["rect_after_restore"] = rect_of(hwnd)
    rep["restore_geom_matches_adopt"] = all(
        abs(a - b) <= 2 for a, b in zip(rep["rect_after_restore"], rep["rect_after_adopt"])
    )
    rep["zoomed_after_restore"] = is_zoomed(hwnd)

    # exited → 占位窗自动关
    t_close = time.time()
    rep["post_close_ret"] = post_close(hwnd)
    rep["exited_logged"] = False
    rep["terminal_state_actual"] = None
    try:
        ln = wait_log_after(t_close, r"state=(exited|orphaned)", timeout=15)
        rep["exited_logged"] = "state=exited" in ln
        rep["terminal_state_actual"] = "orphaned" if "orphaned" in ln else "exited"
    except TimeoutError:
        pass
    # 轮询占位卡回落
    tf_ok = False
    t0 = time.time()
    while time.time() - t0 < 12:
        if tp_count() <= before:
            tf_ok = True
            break
        time.sleep(1.0)
    rep["tp_after_exit"] = tp_count()

    rep["checks"] = {
        "adopted": rep["tp_after_adopt"] == before + 1,
        "tier_expect": rep["tier_actual"] == tier_expect,
        "max_sync": rep["zoomed_after_max"] and rep["max_broadcast"] is not None,
        "min_guard": rep["zoomed_after_min_restore"] and rep["min_guard_holds"],
        "restore_geom": (not rep["zoomed_after_restore"]) and rep["restore_geom_matches_adopt"],
        "terminal_state": ("orphaned" in terminal_expect and rep.get("terminal_state_actual") == "orphaned") or (rep["exited_logged"] and tf_ok),
        "final_clean": rep["tp_after_exit"] == before if rep.get("terminal_state_actual") != "orphaned" else rep["tp_after_exit"] == before + 1,
    }
    rep["pass"] = all(rep["checks"].values())
    print(json.dumps(rep, ensure_ascii=False, indent=1))
    return 0 if rep["pass"] else 1


if __name__ == "__main__":
    sys.exit(main())
