# -*- coding: utf-8 -*-
"""
VTaskBoard — 独立任务板软件（单文件 .exe）
· 任务源 = 同目录 taskboard.md（MD 任务块，人可编辑，AI 可读写）
· 软件内渲染任务卡 + 实时进度；AI 通过 HTTP API 领单/完成/阻塞/退回
· 所有状态变化实时写回 taskboard.md（原子写）
"""
import hashlib
import json, os, re, subprocess, sys, threading, time, urllib.parse, webbrowser
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

APP_DIR = getattr(sys, "_MEIPASS2", os.path.dirname(os.path.abspath(__file__)))
if getattr(sys, "frozen", False):
    APP_DIR = os.path.dirname(sys.executable)
MD_PATH = None
for _cand in (os.path.join(APP_DIR, "taskboard.md"),
              os.path.join(os.path.dirname(APP_DIR), "taskboard.md")):
    if os.path.exists(_cand):
        MD_PATH = _cand
        break
if MD_PATH is None:
    MD_PATH = os.path.join(APP_DIR, "taskboard.md")
PORT = 8767
LOCK = threading.Lock()

# ---------- Wallpaper Engine 壁纸自动识别 ----------
WE_APPID = "431960"
_WE_CACHE = {"t": 0.0, "items": []}

def _steam_libraries():
    """从 Steam libraryfolders.vdf 发现所有 Steam 库根目录"""
    cands = [
        os.environ.get("PROGRAMFILES(X86)", r"C:\Program Files (x86)") + r"\Steam\steamapps\libraryfolders.vdf",
        r"C:\Steam\steamapps\libraryfolders.vdf",
        r"D:\Steam\steamapps\libraryfolders.vdf",
        r"E:\Steam\steamapps\libraryfolders.vdf",
    ]
    for vdf in cands:
        if os.path.isfile(vdf):
            libs, cur = [], None
            try:
                with open(vdf, encoding="utf-8", errors="replace") as f:
                    for line in f:
                        m = re.match(r'\s*"path"\s+"(.+?)"', line)
                        if m:
                            p = m.group(1).replace("\\\\", "\\")
                            if p not in libs:
                                libs.append(p)
            except OSError:
                pass
            if libs:
                return libs
    return [os.path.dirname(os.path.dirname(cands[0]))] if cands else []

def we_root():
    """Wallpaper Engine 创意工坊内容目录（431960）"""
    for lib in _steam_libraries():
        d = os.path.join(lib, "steamapps", "workshop", "content", WE_APPID)
        if os.path.isdir(d):
            return d
    return None

_HD_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "_wehd")

def _display_px():
    """壁纸实际显示区域的物理像素尺寸（宽, 高）。

    必须用物理像素而非 CSS 逻辑像素：壁纸元素由 Edge/Chromium 以 devicePixelRatio
    栅格化，125% 缩放下 CSS 1536x864 对应物理 1920x1080。若按逻辑像素出图，
    浏览器会再放大 1.25 倍插值 —— 那是"越清晰越糊"的根因。
    PER_MONITOR_AWARE 进程下 GetSystemMetrics 返回物理像素；DPI 不可用时回退 1920x1080。
    """
    try:
        if _u32:
            w = _u32.GetSystemMetrics(0)      # SM_CXSCREEN：主屏物理宽（DPI 感知）
            h = _u32.GetSystemMetrics(1)      # SM_CYSCREEN：主屏物理高
            if w > 0 and h > 0:
                return w, h
    except (AttributeError, OSError):
        pass
    return 1920, 1080

def _cover_to(im, tw, th):
    """等比 cover 预裁切到目标宽高比（与 CSS object-fit:cover 语义一致，居中裁切）。

    为什么必须做：官方 preview 是 600x600 / 1024x1024 方形图，而显示区是 16:9。
    若先拉成 tw x tw 方形，浏览器再 cover 裁到 tw x th，等效于「1.875 倍放大 + 一次
    额外的重采样」；实测像素浪费 1.78 倍。先按比例裁掉多余轴、再一次性缩放到
    显示尺寸，浏览器端就只剩一次等比映射，等于把插值损失降到最低。
    """
    s = max(tw / im.width, th / im.height)          # cover 缩放系数
    nw, nh = max(tw, round(im.width * s)), max(th, round(im.height * s))
    if (nw, nh) != im.size:
        im = im.resize((nw, nh), _PImage.LANCZOS)
    if nw > tw or nh > th:                            # 居中裁掉超出轴
        l, t = (nw - tw) // 2, (nh - th) // 2
        im = im.crop((l, t, l + tw, t + th))
    return im

def _wehd_item(pid, pdir):
    """官方 preview.jpg（600~1024 方形小图）→ HD 增强版（按显示区宽高比 cover 预裁切
    + 一次性缩放到显示物理尺寸 + 轻锐化）。

    输出与显示区同比例 => 浏览器 object-fit:cover 退化为 1:1 映射，不再二次重采样，
    呈现效果与原生预览一致。4:4:4 无色度抽样保住红蓝边缘（4:2:0 会抹掉色度细节）。
    幂等缓存 _wehd/<pid>.jpg；生成失败返回 None（前端回退原 preview）。
    缓存键带目标尺寸后缀：换分辨率/换屏不会命中旧尺寸的图。"""
    if not _PIL:
        return None
    src = os.path.join(pdir, "preview.jpg")
    if not os.path.isfile(src):
        return None
    tw, th = _display_px()
    out = os.path.join(_HD_DIR, "%s_%dx%d.jpg" % (pid, tw, th))
    try:
        if os.path.isfile(out) and os.path.getmtime(out) >= os.path.getmtime(src):
            return "/wehd/%s_%dx%d.jpg" % (pid, tw, th)
        os.makedirs(_HD_DIR, exist_ok=True)
        with _PImage.open(src) as im:
            im = im.convert("RGB")
            im = _cover_to(im, tw, th)
            # 仅在确有放大时锐化：缩小或 1:1 时锐化只会制造振铃/白边，反而劣化
            if im.width < tw or im.height < th:
                im = im.resize((tw, th), _PImage.LANCZOS)
                im = im.filter(_PImageFilter.UnsharpMask(radius=1.2, percent=55, threshold=3))
            im.save(out, "JPEG", quality=95, subsampling=0, optimize=True)
        return "/wehd/%s_%dx%d.jpg" % (pid, tw, th)
    except (OSError, ValueError):
        return None

def we_wallpapers():
    """扫描 WE 创意工坊 project.json → video(webm/mp4) / web(iframe) / scene(静帧)"""
    root = we_root()
    if not root:
        return []
    if _WE_CACHE["items"] and time.time() - _WE_CACHE["t"] < 60:
        return _WE_CACHE["items"]
    items = []
    try:
        proj_ids = sorted(os.listdir(root))
    except OSError:
        return []
    for pid in proj_ids:
        pdir = os.path.join(root, pid)
        pj = os.path.join(pdir, "project.json")
        if not os.path.isfile(pj):
            continue
        try:
            with open(pj, encoding="utf-8-sig", errors="replace") as f:
                meta = json.load(f)
        except (OSError, ValueError):
            continue
        typ = (meta.get("type") or "").lower()
        title = (meta.get("title") or pid).strip() or pid
        preview = "/we/%s/preview.jpg" % pid if os.path.isfile(os.path.join(pdir, "preview.jpg")) else None
        if typ == "video":
            fn = meta.get("file") or ""
            if os.path.isfile(os.path.join(pdir, fn)):
                items.append({"id": pid, "name": title, "type": "video",
                              "url": "/we/%s/%s" % (pid, fn.replace("\\", "/")), "preview": preview})
        elif typ == "web":
            fn = meta.get("file") or ""
            if os.path.isfile(os.path.join(pdir, fn)):
                items.append({"id": pid, "name": title, "type": "web",
                              "url": "/we/%s/%s" % (pid, fn.replace("\\", "/")), "preview": preview})
        elif typ == "scene":
            # scene 壁纸：preview.gif 本身是官方生成的动态预览，可直接动起来；
            # 仅 jpg 的项目标注「场景预览」静态呈现（scene.pkg 需 WE 运行时，无法原生渲染）
            has_jpg = os.path.isfile(os.path.join(pdir, "preview.jpg"))
            has_gif = os.path.isfile(os.path.join(pdir, "preview.gif"))
            if has_gif:
                items.append({"id": pid, "name": title, "type": "gif",
                              "url": "/we/%s/preview.gif" % pid,
                              "preview": "/we/%s/preview.jpg" % pid if has_jpg else "/we/%s/preview.gif" % pid})
            elif has_jpg:
                # scene 壁纸收敛（2026-10-03 终判）：实时取帧所有 GDI/DWM 路线全灭——
                # 屏幕抓取必混入遮挡窗口（旧探针抓到抽屉芯片=满屏巨大按钮事故），
                # 沉底探针读到的同样是屏幕合成（DWM 缩略图不进窗口表面/PrintWindow 全白），
                # WGC/csc 在本环境不可达。板内呈现=官方预览的 HD 增强版（Lanczos 放大到
                # 原生屏宽+锐化）；原生分辨率真动画走芯片上的「🖥 桌面」按钮（WE 真渲染）。
                hd = _wehd_item(pid, pdir)
                items.append({"id": pid, "name": title, "type": "live",
                              "url": hd or "/we/%s/preview.jpg" % pid,
                              "fallback": "/we/%s/preview.jpg" % pid,
                              "preview": "/we/%s/preview.jpg" % pid})
    _WE_CACHE["t"] = time.time()
    _WE_CACHE["items"] = items
    return items

# ---------- WE 运行时取帧桥（scene 壁纸深度集成）----------
# 原理：WE 把场景壁纸渲染在桌面 WorkerW 层，用 PrintWindow(PW_RENDERFULLCONTENT)
# 直接抓取该窗口实时画面，编码 MJPEG 流给前端 —— 场景壁纸即在本软件里真动态。
import ctypes
from ctypes import wintypes as _wt

try:
    from PIL import Image as _PImage
    from PIL import ImageFilter as _PImageFilter
    _PIL = True
except ImportError:
    _PIL = False

_u32 = ctypes.windll.user32 if os.name == "nt" else None
_g32 = ctypes.windll.gdi32 if os.name == "nt" else None
_k32 = ctypes.windll.kernel32 if os.name == "nt" else None
_dwm = ctypes.windll.dwmapi if os.name == "nt" else None
if _u32:
    try:  # DPI 感知：取帧按物理像素（4K 屏 = 真 4K 画质）
        ctypes.windll.shcore.SetProcessDpiAwareness(2)
    except (AttributeError, OSError):
        try:
            _u32.SetProcessDPIAware()
        except OSError:
            pass
PW_RENDERFULLCONTENT = 2
_LIVE = {"hwnd": 0, "t": 0.0, "fail": 0.0}

class _BMIH(ctypes.Structure):
    _fields_ = [("biSize", ctypes.c_uint32), ("biWidth", ctypes.c_int32),
                ("biHeight", ctypes.c_int32), ("biPlanes", ctypes.c_uint16),
                ("biBitCount", ctypes.c_uint16), ("biCompression", ctypes.c_uint32),
                ("biSizeImage", ctypes.c_uint32), ("biXPelsPerMeter", ctypes.c_int32),
                ("biYPelsPerMeter", ctypes.c_int32), ("biClrUsed", ctypes.c_uint32),
                ("biClrImportant", ctypes.c_uint32)]

def we_wallpaper_hwnd():
    """找 WE 自己的渲染窗 WPEDesktopDX11Window（D3D 交换链，DWM 缩略图可实时取）；
    找不到退回 WorkerW/Progman"""
    if not _u32:
        return 0
    now = time.time()
    if _LIVE["hwnd"] and _u32.IsWindow(_LIVE["hwnd"]) and now - _LIVE["t"] < 5:
        return _LIVE["hwnd"]
    hit = []
    prog = _u32.FindWindowW("Progman", None)
    # 路径1：Progman → WorkerW → WPEDesktopDX11Window（WE 本机实证挂载点）
    if prog:
        wk = _u32.FindWindowExW(prog, 0, "WorkerW", None)
        while wk:
            wpe = _u32.FindWindowExW(wk, 0, "WPEDesktopDX11Window", None)
            if wpe:
                hit.append(wpe)
                break
            wk = _u32.FindWindowExW(prog, wk, "WorkerW", None)
    # 路径2：顶层 WorkerW 下的 WPE 子窗
    if not hit:
        w = 0
        while True:
            w = _u32.FindWindowExW(0, w, "WorkerW", None)
            if not w:
                break
            wpe = _u32.FindWindowExW(w, 0, "WPEDesktopDX11Window", None)
            if wpe:
                hit.append(wpe)
                break
    hwnd = hit[0] if hit else (prog or 0)
    _LIVE.update(hwnd=hwnd, t=now)
    return hwnd

# ---------- WE 实况窗（DWM 缩略图原生实时预览） ----------
# 原理：DwmRegisterThumbnail(实况窗, Progman)——即任务栏预览的同款系统合成机制，
# DWM 直接把正在播放的桌面壁纸逐帧合成到实况窗客户区 = 原生分辨率原生帧率的实时画面。
# 这不是截屏（PrintWindow/BitBlt 全部取不到缩略内容），而是显示级合成，无遮挡污染。
class _LVWIN:
    hwnd = 0
    thumb = None
    thr = None
    on = False

def _lv_rect():
    sw, sh = _u32.GetSystemMetrics(0), _u32.GetSystemMetrics(1)
    w = min(560, sw // 3)
    h = round(w * 9 / 16)
    return sw - w - 28, sh - h - 100, w, h

def _lv_thread():
    try:
        sw, sh = _u32.GetSystemMetrics(0), _u32.GetSystemMetrics(1)
        x, y, w, h = _lv_rect()
        # 屏幕物理像素换算（服务进程非 DPI aware 时 GetSystemMetrics 返回虚拟化值，先拉起感知）
        try:
            ctypes.windll.shcore.SetProcessDpiAwareness(2)
        except Exception:
            try:
                _u32.SetProcessDPIAware()
            except Exception:
                pass
        sw, sh = _u32.GetSystemMetrics(0), _u32.GetSystemMetrics(1)
        x, y, w, h = _lv_rect()
        hwnd = _u32.CreateWindowExW(0x08000081, "STATIC", "VTaskLiveView", 0x80000000,
                                    x, y, w, h, 0, 0, 0, None)  # TOOLWINDOW|NOACTIVATE|TOPMOST
        if not hwnd:
            return
        _LVWIN.hwnd = hwnd
        _u32.SetWindowPos(hwnd, -1, 0, 0, 0, 0, 0x0053)  # TOPMOST|NOSIZE|NOMOVE|SHOWWINDOW
        _u32.ShowWindow(hwnd, 5)
        src = _u32.FindWindowW("Progman", None) or we_wallpaper_hwnd()
        if src:
            th = _wt.HANDLE()
            if _dwm.DwmRegisterThumbnail(hwnd, src, ctypes.byref(th)) == 0:
                _LVWIN.thumb = th.value
                class _TP(ctypes.Structure):
                    _fields_ = [("dwFlags", _wt.DWORD), ("rcDestination", _wt.RECT),
                                ("rcSource", _wt.RECT), ("opacity", ctypes.c_byte),
                                ("fVisible", _wt.BOOL), ("fSourceClientAreaOnly", _wt.BOOL)]
                p = _TP()
                p.rcDestination = _wt.RECT(0, 0, w, h)
                p.fVisible = True
                p.fSourceClientAreaOnly = True
                p.opacity = 255
                _dwm.DwmUpdateThumbnailProperties(th, ctypes.byref(p))
        msg = _wt.MSG()
        while _LVWIN.on:
            while _u32.PeekMessageW(ctypes.byref(msg), 0, 0, 0, 1):
                _u32.TranslateMessage(ctypes.byref(msg))
                _u32.DispatchMessageW(ctypes.byref(msg))
            time.sleep(0.05)
    finally:
        if _LVWIN.thumb:
            for nm in ("DwmUnRegisterThumbnail", "DwmUnregisterThumbnail"):
                fn = getattr(_dwm, nm, None)
                if fn:
                    try:
                        fn(_LVWIN.thumb)
                    except OSError:
                        pass
                    break
            _LVWIN.thumb = None
        if _LVWIN.hwnd:
            _u32.DestroyWindow(_LVWIN.hwnd)
            _LVWIN.hwnd = 0

def we_liveview(on):
    """开关实况小窗；返回是否成功"""
    if not (_u32 and _g32 and _dwm):
        return False
    if on:
        if _LVWIN.thr and _LVWIN.thr.is_alive() and _LVWIN.on:
            return True
        _LVWIN.on = True
        _LVWIN.thr = threading.Thread(target=_lv_thread, daemon=True)
        _LVWIN.thr.start()
        threading.Thread(target=we_cmd, args=("hideIcons",), daemon=True).start()
        return True
    was = _LVWIN.on
    _LVWIN.on = False
    if was:
        threading.Thread(target=we_cmd, args=("showIcons",), daemon=True).start()
    return True

def _find_window_rect(cls_name=None, own_pid=False):
    """按类名/属主进程找可见窗口矩形（找不到返回 None）"""
    hits = []
    if own_pid:
        proto = ctypes.WINFUNCTYPE(_wt.BOOL, _wt.HWND, _wt.LPARAM)
        me = _k32.GetCurrentProcessId()

        def cb(h, lp):
            pid = _wt.DWORD()
            _u32.GetWindowThreadProcessId(h, ctypes.byref(pid))
            if pid.value == me and _u32.IsWindowVisible(h):
                rc = _wt.RECT()
                _u32.GetWindowRect(h, ctypes.byref(rc))
                if (rc.right - rc.left) > 200 and (rc.bottom - rc.top) > 150:
                    hits.append((rc.right - rc.left) * (rc.bottom - rc.top))
                    hits.append((rc.left, rc.top, rc.right, rc.bottom))
            return True
        _u32.EnumWindows(proto(cb), 0)
    else:
        h = _u32.FindWindowW(cls_name, None)
        if h and _u32.IsWindowVisible(h):
            rc = _wt.RECT()
            _u32.GetWindowRect(h, ctypes.byref(rc))
            hits = [(rc.right - rc.left) * (rc.bottom - rc.top), (rc.left, rc.top, rc.right, rc.bottom)]
    if len(hits) >= 2:
        return hits[1]
    return None

class _LIVE_PROBE:
    """DWM 缩略图探针（原生分辨率·沉底隐形）：
    探针窗=全屏原生像素大小的 STATIC 窗，沉到 Z 序最底（被本应用/其他窗口盖住，用户不可见）。
    DwmRegisterThumbnail(探针, 源窗) 把壁纸动画实时合成进探针的窗口重定向表面；
    实证（2026-10-03）：窗口被不透明窗完全盖住时，GetWindowDC(探针)+BitBlt 读到的
    仍是与可见时逐像素相同的缩略图内容（DWM 持续合成），而屏幕 DC 是遮挡物——
    因此抓探针自身表面 = 原生全分辨率真动画 + 零画面污染。"""
    hwnd = 0
    thumb = None
    rect = (0, 0, 1280, 720)
    shown = False

def _probe_ensure():
    """惰性创建探针窗（全屏原生像素，沉底）+ 注册 DWM 缩略图（失败返回 False）"""
    if not (_u32 and _g32 and _dwm):
        return False
    if _LIVE_PROBE.hwnd and _u32.IsWindow(_LIVE_PROBE.hwnd):
        return _probe_retarget()
    sw, sh = _u32.GetSystemMetrics(0), _u32.GetSystemMetrics(1)
    _LIVE_PROBE.rect = (0, 0, sw, sh)
    WS_POPUP, WS_EX = 0x80000000, 0x08000080  # TOOLWINDOW|TRANSPARENT|NOACTIVATE|TOPMOST
    hwnd = _u32.CreateWindowExW(WS_EX, "STATIC", "VTaskLiveProbe", WS_POPUP,
                                0, 0, sw, sh, 0, 0, 0, None)
    if not hwnd:
        return False
    _LIVE_PROBE.hwnd = hwnd
    if not _probe_retarget():
        return False
    # 沉底：藏在所有普通窗口之下（表面合成不受遮挡影响，见类注释）
    _u32.SetWindowPos(hwnd, 1, 0, 0, 0, 0, 0x0013)  # HWND_BOTTOM|NOMOVE|NOSIZE|NOACTIVATE
    return True

def _probe_retarget():
    """（重新）注册缩略图到当前 WPE 渲染窗；子窗被 DWM 拒绝（E_INVALIDARG 实证）时
    回退顶层 Progman——Progman 缩略图含整棵 WorkerW/WPE 子树，即完整实时壁纸"""
    if not _LIVE_PROBE.hwnd:
        return False
    candidates = [we_wallpaper_hwnd()]
    prog = _u32.FindWindowW("Progman", None)
    if prog:
        candidates.append(prog)
    for src in candidates:
        if not src or not _u32.IsWindow(src):
            continue
        if _LIVE_PROBE.thumb:
            for nm in ("DwmUnRegisterThumbnail", "DwmUnregisterThumbnail"):
                fn = getattr(_dwm, nm, None)
                if fn:
                    try:
                        fn(_LIVE_PROBE.thumb)
                    except OSError:
                        pass
                    break
            _LIVE_PROBE.thumb = None
        thumb = _wt.HANDLE()
        if _dwm.DwmRegisterThumbnail(_LIVE_PROBE.hwnd, src, ctypes.byref(thumb)) != 0:
            continue
        _LIVE_PROBE.thumb = thumb.value
        class DWM_THUMB_PROPS(ctypes.Structure):
            _fields_ = [("fSource", _wt.DWORD), ("rcDestination", _wt.RECT),
                        ("rcSource", _wt.RECT), ("opacity", ctypes.c_byte),
                        ("fVisible", _wt.BOOL), ("fSourceClientAreaOnly", _wt.BOOL)]
        props = DWM_THUMB_PROPS()
        props.rcDestination = _wt.RECT(*_probe_client_rect())
        props.fSourceClientAreaOnly = True
        props.fVisible = True
        props.opacity = 255
        _dwm.DwmUpdateThumbnailProperties(_LIVE_PROBE.thumb, ctypes.byref(props))
        _u32.SetWindowPos(_LIVE_PROBE.hwnd, 1, 0, 0, 0, 0, 0x0053)  # BOTTOM|NOSIZE|NOMOVE|SHOWWINDOW
        _u32.ShowWindow(_LIVE_PROBE.hwnd, 5)
        _LIVE_PROBE.shown = True
        return True
    return False

def _probe_client_rect():
    x, y, r, b = _LIVE_PROBE.rect
    return (0, 0, r - x, b - y)

def _probe_hide():
    if _LIVE_PROBE.hwnd and _LIVE_PROBE.shown:
        _u32.ShowWindow(_LIVE_PROBE.hwnd, 0)
        _LIVE_PROBE.shown = False

def we_live_frame(maxw=0):
    """经 DWM 缩略图探针抓 WE 壁纸真动画一帧 → JPEG；探针不可用返回 None。
    关键：读探针自身的窗口重定向表面（GetWindowDC(探针)），不是屏幕——
    探针沉底被完全遮挡时表面仍被 DWM 持续合成（2026-10-03 实证），
    抓到的永远是壁纸，绝不混入任何遮挡窗口（镜中镜根治）"""
    if not (_PIL and _u32 and _g32):
        return None
    if not _probe_ensure():
        return None
    x, y, r, b = _LIVE_PROBE.rect
    w, h = r - x, b - y
    wdc = _u32.GetWindowDC(_LIVE_PROBE.hwnd)
    if not wdc:
        return None
    mem = _g32.CreateCompatibleDC(wdc)
    bmp = _g32.CreateCompatibleBitmap(wdc, w, h)
    old = _g32.SelectObject(mem, bmp)
    ok = _g32.BitBlt(mem, 0, 0, w, h, wdc, 0, 0, 0x00CC0020)
    bmi = _BMIH()
    bmi.biSize = ctypes.sizeof(_BMIH)
    bmi.biWidth, bmi.biHeight = w, -h
    bmi.biPlanes, bmi.biBitCount = 1, 32
    buf = ctypes.create_string_buffer(w * h * 4)
    got = _g32.GetDIBits(mem, bmp, 0, h, buf, ctypes.byref(bmi), 0) if ok else False
    _g32.SelectObject(mem, old)
    _g32.DeleteObject(bmp)
    _g32.DeleteDC(mem)
    _u32.ReleaseDC(_LIVE_PROBE.hwnd, wdc)
    if not got:
        return None
    img = _PImage.frombuffer("RGB", (w, h), buf, "raw", "BGRX", 0, 1)
    if maxw and img.width > maxw:
        img = img.resize((maxw, round(img.height * maxw / img.width)), _PImage.BILINEAR)
    return _jpg(img, 85)

def _jpg(img, q=88):
    import io
    b = io.BytesIO()
    img.save(b, "JPEG", quality=q)
    return b.getvalue()

# ---------- WE 控制通道（-control 走运行中实例的 WPXCMD_ 命名管道）----------
_LIVE_REFCOUNT = {"n": 0, "lock": threading.Lock()}
_LIVE_FRAME = {"jpg": None, "t": 0.0, "sig": None, "stall": 0, "lock": threading.Lock()}
_PREVIEW_TIMER = {"t": None, "lock": threading.Lock()}
_CAP_MAXW = 0      # 0=原生全分辨率取帧（1920×1080 物理像素，探针表面直读无遮挡）
_CAP_DT = 0.1      # 帧间隔 → ~10fps（全分辨率编码更重，10fps 已流畅）

def _capture_loop():
    """常驻捕获线程：抓取+降采样编码，流端只推送。异常自愈（线程崩=画面永久冻结）"""
    n = 0
    idle = 0
    while True:
        try:
            with _LIVE_REFCOUNT["lock"]:
                active = _LIVE_REFCOUNT["n"] > 0
            if not active:
                idle += 1
                if idle == 33 and _LIVE.get("restore"):   # 无客户端 ~10s → 兜底还原一次
                    threading.Thread(target=we_restore_desktop, daemon=True).start()
                time.sleep(0.3)
                continue
            idle = 0
            n += 1
            if n % 50 == 0:                    # 每 ~5s 重assert图标隐藏（防 Explorer 重绘回显）
                threading.Thread(target=we_cmd, args=("hideIcons",), daemon=True).start()
            f = we_live_frame(_CAP_MAXW)       # 降采样：降 CPU/内存带宽，画面依旧清晰
            if f is not None:
                sig = hashlib.md5(f).hexdigest()
                with _LIVE_FRAME["lock"]:
                    stall = _LIVE_FRAME["stall"] + 1 if sig == _LIVE_FRAME["sig"] else 0
                    _LIVE_FRAME["sig"] = sig
                    _LIVE_FRAME["stall"] = stall
                    _LIVE_FRAME["jpg"] = f
                    _LIVE_FRAME["t"] = time.time()
                time.sleep(_CAP_DT)            # ~12fps：风景场景流畅足够，CPU 减半以上
            else:
                time.sleep(0.25)
        except Exception:
            time.sleep(0.5)                    # 任何异常不杀线程，下一轮自愈

threading.Thread(target=_capture_loop, daemon=True).start()

def we_exe():
    """wallpaper64.exe 路径：由 workshop 根上溯 Steam 库 → steamapps/common/wallpaper_engine"""
    root = we_root()
    if root:
        lib = root
        for _ in range(4):
            lib = os.path.dirname(lib)          # 431960→content→workshop→steamapps→库根
        cand = os.path.join(lib, "steamapps", "common", "wallpaper_engine", "wallpaper64.exe")
        if os.path.isfile(cand):
            return cand
    for c in (r"D:\steam\steamapps\common\wallpaper_engine\wallpaper64.exe",
              r"C:\Program Files (x86)\Steam\steamapps\common\wallpaper_engine\wallpaper64.exe"):
        if os.path.isfile(c):
            return c
    return None

def we_cmd(*args):
    exe = we_exe()
    if not exe:
        return False
    try:
        r = subprocess.run([exe, "-control"] + list(args),
                           capture_output=True, timeout=15)
        return r.returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        return False

def _find_key(obj, name):
    """递归找第一个同名键的值（WE config.json 结构时变，位置不固定）"""
    if isinstance(obj, dict):
        for k, v in obj.items():
            if k == name:
                return v
            r = _find_key(v, name)
            if r is not None:
                return r
    elif isinstance(obj, list):
        for v in obj:
            r = _find_key(v, name)
            if r is not None:
                return r
    return None

def we_desktop_wallpaper():
    """读 WE config.json 里用户当前桌面壁纸文件（Monitor0），用于临时渲染后还原"""
    exe = we_exe()
    if not exe:
        return None
    cfg = os.path.join(os.path.dirname(exe), "config.json")
    try:
        with open(cfg, encoding="utf-8") as f:
            data = json.load(f)
        # 还原目标优先取「用户最近在 WE UI 里主动设置的一张」（wallpaperconfigrecent[0]），
        # 防止我们此前临时切换留下的 selectedwallpapers 脏状态被当成用户原壁纸
        rec = _find_key(data, "wallpaperconfigrecent") or []
        if rec:
            rsw = _find_key(rec[0], "selectedwallpapers") or {}
            f = next(iter(rsw.values()), {}).get("file")
            if f:
                return f
        sw = _find_key(data, "selectedwallpapers") or {}
        mon = next(iter(sw.values()), {})
        return mon.get("file")
    except (OSError, ValueError, StopIteration):
        return None
    return None

_LIVE["restore"] = None   # 用户原桌面壁纸（还原用）；_LIVE 已含 hwnd/t/fail

def we_open_scene(pid):
    """把桌面 WE 临时切到指定场景工程（真渲染该 scene.pkg）：
    首次切换前记录用户原壁纸，镜像结束自动还原 —— Windows 桌面最终不被改变"""
    root = we_root()
    pkg = os.path.join(root or "", pid, "scene.pkg")
    if not os.path.isfile(pkg):
        return False
    with _LIVE_REFCOUNT["lock"]:
        first_switch = _LIVE["restore"] is None
        if first_switch:
            _LIVE["restore"] = we_desktop_wallpaper()
    ok = we_cmd("openWallpaper", "-file", pkg)
    we_cmd("hideIcons")
    return ok

def we_restore_desktop():
    """把桌面 WE 还原为用户原壁纸 + 恢复图标"""
    f = _LIVE.get("restore")
    if f and os.path.isfile(f):
        we_cmd("openWallpaper", "-file", f)
    we_cmd("showIcons")

def we_live_client_begin():
    """首个取帧客户端接入 → 隐藏桌面图标（镜像画面纯净）"""
    with _LIVE_REFCOUNT["lock"]:
        _LIVE_REFCOUNT["n"] += 1
        first = _LIVE_REFCOUNT["n"] == 1
    if first:
        threading.Thread(target=we_cmd, args=("hideIcons",), daemon=True).start()

def we_live_client_end():
    """取帧客户端全部断开 → 还原用户原桌面壁纸 + 恢复图标"""
    with _LIVE_REFCOUNT["lock"]:
        _LIVE_REFCOUNT["n"] = max(0, _LIVE_REFCOUNT["n"] - 1)
        last = _LIVE_REFCOUNT["n"] == 0
    if last:
        threading.Thread(target=we_restore_desktop, daemon=True).start()

import atexit as _atexit
_atexit.register(we_restore_desktop)
threading.Timer(2.0, lambda: we_restore_desktop()).start()  # 启动兜底：还原用户原壁纸与图标

BLOCK_HEAD = re.compile(r"^## \[([A-Za-z0-9_\-\.]+)\]\s*(.+)$")
FIELD = re.compile(r"^-\s+(册|域|行数|前置|书路径|验收|状态|领取人|结果|时间):\s*(.*)$")

def parse_md():
    """解析 taskboard.md → tasks 列表 + 原始行"""
    with open(MD_PATH, encoding="utf-8") as f:
        lines = f.read().split("\n")
    tasks, cur, i = [], None, 0
    while i < len(lines):
        m = BLOCK_HEAD.match(lines[i])
        if m:
            cur = {"_start": i, "_end": i + 1, "id": m.group(1), "title": m.group(2).strip(),
                   "册": "", "域": "", "行数": "", "前置": "", "书路径": "", "验收": "",
                   "状态": "待领", "领取人": "", "结果": "", "时间": ""}
            tasks.append(cur)
        elif cur is not None:
            fm = FIELD.match(lines[i])
            if fm:
                cur[fm.group(1)] = fm.group(2).strip()
                cur["_end"] = i + 1
            elif lines[i].strip() == "" and cur["_end"] == i:
                cur["_end"] = i + 1
            else:
                cur = None  # 离开块
        i += 1
    for t in tasks:
        t["langs"], t["files"] = infer_langs(t)
    return lines, tasks

def write_back(lines, tasks):
    """把任务状态写回原始行并原子落盘"""
    for t in tasks:
        block_lines = lines[t["_start"]:t["_end"]]
        for key in ("状态", "领取人", "结果", "时间"):
            pat = re.compile(r"^-\s+" + key + r":\s*(.*)$")
            newv = t.get(key, "")
            hit = False
            for j in range(len(block_lines)):
                if pat.match(block_lines[j]):
                    block_lines[j] = f"- {key}: {newv}"
                    hit = True
                    break
            if not hit:
                block_lines.append(f"- {key}: {newv}")
        lines[t["_start"]:t["_end"]] = block_lines
    tmp = MD_PATH + ".tmp"
    with open(tmp, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines))
    os.replace(tmp, MD_PATH)

def done_ids(tasks):
    return {t["id"] for t in tasks if t["状态"] == "已完成"}

# ---------- 产物文件与编程语言识别（每语言独立图标由前端渲染） ----------
FILE_RE = re.compile(r"[A-Za-z0-9_\-\u4e00-\u9fff]+\.(?:py|rs|tsx?|jsx?|html?|css|ps1|bat|cmd|toml|json|sh)\b")
EXT_LANG = {"py": "Python", "rs": "Rust", "ts": "TypeScript", "tsx": "TypeScript",
            "js": "JavaScript", "jsx": "JavaScript", "html": "HTML", "css": "CSS",
            "ps1": "PowerShell", "bat": "Batch", "cmd": "Batch", "toml": "TOML",
            "json": "JSON", "sh": "Shell", "md": "Markdown"}
BOOK_LANG = {"CoRun": "Rust", "CGPU": "Rust", "VE": "TypeScript"}
LANG_ORDER = ["TypeScript", "JavaScript", "Python", "Rust", "HTML", "CSS",
              "PowerShell", "Batch", "Shell", "JSON", "TOML", "Markdown"]
# 词边界匹配，防 "rust"→"trust"、"css"→"process" 之类误判
KW_RE = re.compile(r"\b(rust|typescript|javascript|python|powershell|html|css|json|shell|batch)\b", re.I)
KW_CANON = {"rust": "Rust", "typescript": "TypeScript", "javascript": "JavaScript",
            "python": "Python", "powershell": "PowerShell", "html": "HTML", "css": "CSS",
            "json": "JSON", "shell": "Shell", "batch": "Batch"}
# Varix 内核线索：命中即把 Rust 提前（内核/驱动/引导/存储类条目必然是内核语言）
KERNEL_RE = re.compile(r"内核|kernel|驱动|driver|引导|boot|AHCI|NVMe|xHCI|中断|中断|页表|调度|syscall|系统调用", re.I)

def infer_langs(t):
    """从标题/域/验收/书路径提取产物文件与语言；册默认语言恒居首（Varix 内核=CoRun/CGPU→Rust、VE→TypeScript）"""
    text = " ".join([t.get("title", ""), t.get("域", ""), t.get("验收", ""), t.get("书路径", "")])
    files = []
    for m in FILE_RE.finditer(text):
        f = m.group(0)
        if f not in files:
            files.append(f)
    default = BOOK_LANG.get(t.get("册", ""))
    langs = [default] if default else []
    for m in KW_RE.finditer(text):
        kw = KW_CANON[m.group(1).lower()]
        if kw not in langs:
            langs.append(kw)
    for f in files:
        lg = EXT_LANG.get(f.rsplit(".", 1)[-1].lower())
        if lg and lg not in langs:
            langs.append(lg)
    if not langs:
        return [], files
    # 内核线索命中 → Rust 恒排最前（即便是 VE 册里引用内核行为的条目也标明主语言归属）
    if "Rust" in langs and KERNEL_RE.search(text):
        langs.remove("Rust"); langs.insert(0, "Rust")
    elif default and default in langs:
        langs.remove(default); langs.insert(0, default)
    rest = [l for l in langs[1:]]
    rest.sort(key=LANG_ORDER.index)
    return [langs[0]] + rest, files

def is_unlocked(t, done):
    return not t.get("前置") or t["前置"] in done

# ---------- 原书详述索引：书路径锚点 → 该条 ~300 字功能详述 ----------
_DETAIL_CACHE = {"key": None, "map": {}}
DETAIL_HEAD = re.compile(r"^#{2,4}\s+([A-Z]{2,12}-[A-Za-z0-9\-]+)\s*[·•]\s*(.+)$")

def _load_detail_map(path):
    """解析一本施工书：### ID · 标题 之后的正文段（到下一个标题为止）"""
    out = {}
    try:
        with open(path, encoding="utf-8") as f:
            lines = f.read().split("\n")
    except OSError:
        return out
    cur, buf = None, []
    for ln in lines:
        hm = DETAIL_HEAD.match(ln)
        if hm:
            if cur:
                out[cur] = "\n".join(buf).strip()
            cur, buf = hm.group(1), []
        elif cur is not None:
            if re.match(r"^#{1,4}\s", ln):          # 下一标题：收束
                out[cur] = "\n".join(buf).strip()
                cur, buf = None, []
            else:
                buf.append(ln)
    if cur:
        out[cur] = "\n".join(buf).strip()
    return out

def book_detail(bookpath, tid):
    """按 书路径#ID 取原书详述段（含 mtime 缓存；多篇书共享一个索引）"""
    p = bookpath.split("#")[0].strip()
    if not p or not os.path.exists(p):
        return ""
    try:
        st = os.stat(p)
        key = (p, st.st_mtime_ns, st.st_size)
    except OSError:
        return ""
    if _DETAIL_CACHE["key"] != key:
        try:
            m = dict(_DETAIL_CACHE["map"])
            m[p] = _load_detail_map(p)
            _DETAIL_CACHE.update(key=key, map=m)
        except OSError:
            pass
    return _DETAIL_CACHE["map"].get(p, {}).get(tid, "")

def snapshot():
    """mtime 缓存：文件没变就不重解析（2 万卡级必备）"""
    global _CACHE
    try:
        st = os.stat(MD_PATH)
        key = (st.st_mtime_ns, st.st_size)
    except OSError:
        key = None
    if _CACHE["key"] != key:
        lines, tasks = parse_md()
        _CACHE.update(key=key, lines=lines, tasks=tasks)
    return _CACHE["lines"], _CACHE["tasks"], [
        {k: v for k, v in t.items() if not k.startswith("_")} for t in _CACHE["tasks"]]

_CACHE = {"key": None, "lines": [], "tasks": []}

class H(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def _json(self, obj, code=200):
        b = json.dumps(obj, ensure_ascii=False).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def do_GET(self):
        if self.path.startswith("/api/health"):
            return self._json({"ok": True, "app": "VTaskBoard", "build": 17, "port": PORT, "md": MD_PATH})
        from urllib.parse import urlparse, parse_qs
        q = parse_qs(urlparse(self.path).query)
        with LOCK:
            _, tasks, pub = snapshot()
            if self.path.startswith("/api/task?"):
                tid = q.get("id", [""])[0]
                t = next((x for x in pub if x["id"] == tid), None)
                if t:
                    bk = q.get("book", [""])[0]
                    scope = [x for x in pub if not bk or x["册"] == bk]
                    t = dict(t, 解锁=is_unlocked(t, done_ids(tasks)),
                             index=next(i for i, x in enumerate(scope) if x["id"] == tid),
                             简介=book_detail(t.get("书路径", ""), tid))
                return self._json({"task": t})
            if self.path.startswith("/api/summary"):
                by_status, by_book, workers = {}, {}, {}
                for t in tasks:
                    s, b = t["状态"], t["册"]
                    by_status[s] = by_status.get(s, 0) + 1
                    bk = by_book.setdefault(b, {"待领": 0, "已领": 0, "阻塞": 0, "已完成": 0, "总": 0})
                    bk[s] = bk.get(s, 0) + 1
                    bk["总"] += 1
                    if s == "已领" and t["领取人"]:
                        workers[t["领取人"]] = workers.get(t["领取人"], 0) + 1
                return self._json({"total": len(tasks), "by_status": by_status, "by_book": by_book,
                                   "workers": workers,
                                   "md": MD_PATH, "port": PORT})
            if self.path.startswith("/api/maptasks"):
                # 一次性返回全部域的任务（每域限量），供导图单请求拉齐
                try:
                    lim = min(300, max(1, int(q.get("limit", ["120"])[0])))
                except ValueError:
                    lim = 120
                stf = q.get("status", [""])[0]
                done = done_ids(tasks)
                out = {}
                for t in tasks:
                    if stf and t["状态"] != stf:
                        continue
                    k = t["册"] + "/" + t["域"]
                    lst = out.setdefault(k, [])
                    if len(lst) < lim:
                        if stf:
                            # 状态过滤模式（BOARD 泳道）：只投影板面需要的字段
                            lst.append({"id": t["id"], "title": t["title"], "状态": t["状态"],
                                        "前置": t.get("前置", ""), "解锁": is_unlocked(t, done),
                                        "langs": t.get("langs", [])})
                        else:
                            lst.append({x: v for x, v in t.items() if not x.startswith("_")}
                                       | {"解锁": is_unlocked(t, done)})
                return self._json({"domains": out, "md": MD_PATH, "port": PORT})
            if self.path.startswith("/api/map"):
                # 导图树：册 → 域 {total, done, active, first（域内第一条）}
                order = {}
                for t in tasks:
                    b, d = t["册"], t["域"]
                    o = order.setdefault(b, {})
                    if d not in o:
                        o[d] = {"d": d, "total": 0, "done": 0, "active": 0, "first": t["id"]}
                    n = o[d]
                    n["total"] += 1
                    if t["状态"] == "已完成":
                        n["done"] += 1
                    elif t["状态"] == "已领":
                        n["active"] += 1
                return self._json({"books": {b: list(o.values()) for b, o in order.items()},
                                   "md": MD_PATH, "port": PORT})
            if self.path.startswith("/api/tasks"):
                # 可选 status/book/domain/limit/offset/unlocked 过滤
                done = done_ids(tasks)
                flt = [t for t in pub if is_unlocked(t, done)] if q.get("unlocked") else pub
                for k, key in (("status", "状态"), ("book", "册"), ("domain", "域")):
                    if q.get(k):
                        v = q[k][0]
                        flt = [t for t in flt if t[key] == v]
                try:
                    off = max(0, int(q.get("offset", ["0"])[0]))
                    lim = min(2000, max(1, int(q.get("limit", ["100000"])[0])))
                except ValueError:
                    off, lim = 0, 100000
                page = [dict(t, 解锁=is_unlocked(t, done)) for t in flt[off:off+lim]]
                ndone = sum(1 for t in pub if t["状态"] == "已完成")
                return self._json({"tasks": page, "total": len(flt), "shown": min(lim, len(flt)),
                                   "done": ndone, "md": MD_PATH, "port": PORT})
        # 壁纸库：/api/wallpapers 列表；/wallpapers/<file> 静态服务（4K 图片 / 循环视频）
        p0 = self.path.split("?")[0]
        if p0 == "/we-live.mjpg":
            # WE 运行时取帧 MJPEG 流（捕获线程持续产出）；必须等到**新鲜帧**（连接后新产出），
            # 陈旧帧=捕获线程已死/冻结，直接 404 让前端回退静态预览
            we_live_client_begin()                 # 先激活捕获线程，再等首帧（防互相等死锁）
            t0 = time.time()
            while True:
                with _LIVE_FRAME["lock"]:
                    ft = _LIVE_FRAME["t"]
                if ft > t0 or time.time() - t0 > 6:
                    break
                time.sleep(0.1)
            with _LIVE_FRAME["lock"]:
                frozen = _LIVE_FRAME["stall"] >= 8      # 源冻结（PrintWindow 缓存等）→ 拒流
                fr = _LIVE_FRAME["jpg"] if (_LIVE_FRAME["t"] > t0 and not frozen) else None
            if fr is None:
                we_live_client_end()
                self.send_response(404); self.end_headers(); return
            # begin 已在等帧前调用，此处不再重复计数
            self.send_response(200)
            self.send_header("Content-Type", "multipart/x-mixed-replace; boundary=frame")
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            try:
                last = 0.0
                while True:
                    with _LIVE_FRAME["lock"]:
                        fr = _LIVE_FRAME["jpg"]
                    if fr is not None and fr is not last:
                        self.wfile.write(b"--frame\r\nContent-Type: image/jpeg\r\nContent-Length: "
                                         + str(len(fr)).encode() + b"\r\n\r\n" + fr + b"\r\n")
                        self.wfile.flush()
                        last = fr
                    time.sleep(0.1)
            except (BrokenPipeError, ConnectionAbortedError, ConnectionResetError, OSError):
                pass
            finally:
                we_live_client_end()
            return
        if p0 == "/api/we/open":
            # 切换 WE 桌面到指定场景工程（真渲染 scene.pkg）
            pid = q.get("id", [""])[0]
            ok = we_open_scene(pid) if pid else False
            return self._json({"ok": ok})
        if p0 == "/api/wallpapers":
            wdir = os.path.join(APP_DIR, "wallpapers")
            os.makedirs(wdir, exist_ok=True)
            IMG = (".jpg", ".jpeg", ".png", ".webp", ".avif", ".bmp")
            VID = (".mp4", ".webm", ".mkv")
            out = []
            try:
                for fn in sorted(os.listdir(wdir)):
                    lo = fn.lower()
                    if lo.endswith(IMG):
                        out.append({"name": fn, "type": "img", "url": "/wallpapers/" + fn})
                    elif lo.endswith(VID):
                        out.append({"name": fn, "type": "video", "url": "/wallpapers/" + fn})
            except OSError:
                pass
            return self._json({"wallpapers": out, "dir": wdir})
        if p0.startswith("/wallpapers/"):
            fn = os.path.basename(p0[len("/wallpapers/"):])
            fp = os.path.join(APP_DIR, "wallpapers", fn)
            if os.path.isfile(fp):
                ext = os.path.splitext(fn)[1].lower()
                ct = {".jpg": "image/jpeg", ".jpeg": "image/jpeg", ".png": "image/png",
                      ".webp": "image/webp", ".avif": "image/avif", ".bmp": "image/bmp",
                      ".mp4": "video/mp4", ".webm": "video/webm", ".mkv": "video/x-matroska"}.get(ext, "application/octet-stream")
                # Range 支持：大视频按需取段，避免整文件读进内存（与 /we/ 行为对齐）
                size = os.path.getsize(fp)
                rng = self.headers.get("Range")
                if rng and rng.startswith("bytes=") and size:
                    a, _, b = rng[6:].partition("-")
                    try:
                        a = int(a or 0)
                    except ValueError:
                        a = 0
                    b = int(b) if b else size - 1
                    b = min(b, size - 1)
                    if a > b or a >= size:      # 越界 -> 416
                        self.send_response(416)
                        self.send_header("Content-Range", "bytes */%d" % size)
                        self.end_headers()
                        return
                    self.send_response(206)
                    self.send_header("Content-Range", "bytes %d-%d/%d" % (a, b, size))
                    self.send_header("Content-Length", str(b - a + 1))
                    self.send_header("Content-Type", ct)
                    self.send_header("Accept-Ranges", "bytes")
                    self.send_header("Cache-Control", "max-age=86400")
                    self.end_headers()
                    with open(fp, "rb") as f:
                        f.seek(a)
                        self.wfile.write(f.read(b - a + 1))
                    return
                with open(fp, "rb") as f:
                    b = f.read()
                self.send_response(200)
                self.send_header("Content-Type", ct)
                self.send_header("Accept-Ranges", "bytes")
                # 图片/视频本体不重编码：原样透传，浏览器解码即原生画质。
                # 素材文件名可能不变但内容被替换过，故用 mtime+size 做 ETag 弱校验。
                self.send_header("Cache-Control", "max-age=86400")
                self.send_header("ETag", 'W/"%x-%x"' % (int(os.path.getmtime(fp)), size))
                self.send_header("Content-Length", str(len(b)))
                self.end_headers()
                return self.wfile.write(b)
            self.send_response(404); self.end_headers(); return
        # 皮肤贴图资产：/ui/<sub>/<file> 静态服务（wallpapers/ui/ 下，官方 MC GUI 贴图等）
        if p0.startswith("/ui/"):
            root = os.path.join(APP_DIR, "wallpapers", "ui")
            rel = urllib.parse.unquote(p0[len("/ui/"):])
            if not rel or ".." in rel:
                self.send_response(404); self.end_headers(); return
            fp = os.path.normpath(os.path.join(root, rel))
            if not fp.startswith(os.path.normpath(root)) or not os.path.isfile(fp):
                self.send_response(404); self.end_headers(); return
            ext = os.path.splitext(fp)[1].lower()
            # 字体 MIME 必须给对：octet-stream 下浏览器会拒载 @font-face，
            # 表现为定制皮肤字体静默回退系统字体（像素风失效）。
            ct = {".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg",
                  ".webp": "image/webp", ".svg": "image/svg+xml",
                  ".ttf": "font/ttf", ".otf": "font/otf", ".woff2": "font/woff2",
                  ".woff": "font/woff"}.get(ext, "application/octet-stream")
            with open(fp, "rb") as f:
                b = f.read()
            self.send_response(200)
            self.send_header("Content-Type", ct)
            self.send_header("Cache-Control", "max-age=86400")
            self.send_header("Content-Length", str(len(b)))
            self.end_headers()
            return self.wfile.write(b)
        # Wallpaper Engine 壁纸自动识别：/api/we/list + /we/<proj>/<file> 服务
        if p0 == "/api/we/list":
            return self._json({"items": we_wallpapers(), "root": we_root(), "note":
                "video=原视频循环；web=网页嵌入；gif=官方动态预览循环；live=场景壁纸经 WE 运行时取帧实时镜像（WE 未运行回退静态预览）"})
        if p0.startswith("/wehd/"):
            rel = urllib.parse.unquote(p0[len("/wehd/"):])
            if not rel or ".." in rel:
                self.send_response(404); self.end_headers(); return
            fp = os.path.normpath(os.path.join(_HD_DIR, rel))
            if not fp.startswith(os.path.normpath(_HD_DIR)) or not os.path.isfile(fp):
                self.send_response(404); self.end_headers(); return
            mtime = int(os.path.getmtime(fp))
            size = os.path.getsize(fp)
            etag = 'W/"%x-%x"' % (mtime, size)
            if self.headers.get("If-None-Match") == etag:
                self.send_response(304)
                self.send_header("ETag", etag)
                self.end_headers()
                return
            self.send_response(200)
            self.send_header("Content-Type", "image/jpeg")
            self.send_header("ETag", etag)
            self.send_header("Content-Length", str(size))
            self.send_header("Cache-Control", "max-age=3600")
            self.end_headers()
            with open(fp, "rb") as f:
                self.wfile.write(f.read())
            return
        if p0.startswith("/we/"):
            root = we_root()
            rel = urllib.parse.unquote(p0[len("/we/"):])
            if not root or not rel or ".." in rel:
                self.send_response(404); self.end_headers(); return
            fp = os.path.normpath(os.path.join(root, rel))
            if not fp.startswith(os.path.normpath(root)) or not os.path.isfile(fp):
                self.send_response(404); self.end_headers(); return
            ext = os.path.splitext(fp)[1].lower()
            ct = {".jpg": "image/jpeg", ".jpeg": "image/jpeg", ".png": "image/png",
                  ".webp": "image/webp", ".gif": "image/gif", ".svg": "image/svg+xml",
                  ".mp4": "video/mp4", ".webm": "video/webm", ".mp3": "audio/mpeg",
                  ".ogg": "audio/ogg", ".wav": "audio/wav",
                  ".html": "text/html; charset=utf-8", ".htm": "text/html; charset=utf-8",
                  ".js": "text/javascript; charset=utf-8", ".mjs": "text/javascript; charset=utf-8",
                  ".css": "text/css; charset=utf-8", ".json": "application/json",
                  ".woff2": "font/woff2", ".woff": "font/woff", ".ttf": "font/ttf",
                  ".txt": "text/plain; charset=utf-8"}.get(ext, "application/octet-stream")
            # Range 支持大视频拖动/循环
            size = os.path.getsize(fp)
            rng = self.headers.get("Range")
            if rng and rng.startswith("bytes="):
                a, _, b = rng[6:].partition("-")
                a = int(a or 0); b = int(b) if b else size - 1
                b = min(b, size - 1)
                self.send_response(206)
                self.send_header("Content-Range", f"bytes {a}-{b}/{size}")
                self.send_header("Content-Length", str(b - a + 1))
                self.send_header("Content-Type", ct)
                self.send_header("Accept-Ranges", "bytes")
                self.end_headers()
                with open(fp, "rb") as f:
                    f.seek(a); self.wfile.write(f.read(b - a + 1))
                return
            self.send_response(200)
            self.send_header("Content-Type", ct)
            self.send_header("Content-Length", str(size))
            self.send_header("Accept-Ranges", "bytes")
            self.end_headers()
            with open(fp, "rb") as f:
                self.wfile.write(f.read())
            return
        # 其余路径 = ui.html / favicon.ico
        if self.path.split("?")[0] == "/favicon.ico":
            fp = os.path.join(APP_DIR, "favicon.ico")
            if os.path.exists(fp):
                with open(fp, "rb") as f:
                    b = f.read()
                self.send_response(200)
                self.send_header("Content-Type", "image/x-icon")
                self.send_header("Content-Length", str(len(b)))
                self.end_headers()
                return self.wfile.write(b)
        try:
            with open(os.path.join(APP_DIR, "ui.html"), encoding="utf-8") as f:
                html = f.read()
        except FileNotFoundError:
            html = "<h1>VTaskBoard</h1><p>ui.html 缺失</p>"
        b = html.encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        try:
            body = json.loads(self.rfile.read(n) or b"{}")
        except Exception:
            return self._json({"ok": False, "err": "bad json"}, 400)
        act = self.path.split("?")[0]
        if act == "/api/we/open":
            from urllib.parse import urlparse as _up, parse_qs as _pq
            _q = _pq(_up(self.path).query)
            pid = _q.get("id", [""])[0]
            ok = we_open_scene(pid) if pid else False
            return self._json({"ok": ok})
        if act == "/api/we/liveview":
            # 实况小窗：DWM 缩略图原生实时预览（任务栏预览同款合成机制）
            on = bool(body.get("on"))
            return self._json({"ok": we_liveview(on)})
        if act == "/api/we/preview":
            # 显式「桌面临时预览」：真渲染 15 分钟后自动还原（不经镜像直接看原生画面）
            from urllib.parse import urlparse as _up, parse_qs as _pq
            _q = _pq(_up(self.path).query)
            pid = _q.get("id", [""])[0]
            ok = we_open_scene(pid) if pid else False
            if ok:
                with _PREVIEW_TIMER["lock"]:
                    if _PREVIEW_TIMER["t"]:
                        _PREVIEW_TIMER["t"].cancel()
                    _PREVIEW_TIMER["t"] = threading.Timer(900, lambda: we_restore_desktop())
                    _PREVIEW_TIMER["t"].daemon = True
                    _PREVIEW_TIMER["t"].start()
            return self._json({"ok": ok})
        with LOCK:
            lines, tasks, _ = snapshot()
            tid = body.get("id")
            t = next((x for x in tasks if x["id"] == tid), None) if tid else None
            if act == "/api/claim":
                done = done_ids(tasks)
                if t:  # 指名领单
                    if t["状态"] != "待领":
                        return self._json({"ok": False, "err": "已被人领取", "status": t["状态"]}, 409)
                    if not is_unlocked(t, done):
                        return self._json({"ok": False, "err": f"前置未完成: {t['前置']}（先完成前置才解锁）"}, 423)
                    t["状态"], t["领取人"] = "已领", body.get("worker", "AI-?")
                    t["时间"] = time.strftime("%m-%d %H:%M")
                else:  # 自动领第一张已解锁待领
                    t = next((x for x in tasks if x["状态"] == "待领" and is_unlocked(x, done)), None)
                    if not t:
                        return self._json({"ok": False, "err": "队列空或全部前置未完成，无可领单"})
                    t["状态"], t["领取人"] = "已领", body.get("worker", "AI-?")
                    t["时间"] = time.strftime("%m-%d %H:%M")
                write_back(lines, tasks)
                return self._json({"ok": True, "task": {k: v for k, v in t.items() if not k.startswith("_")}})
            if act == "/api/release_worker":
                # 取消 AI 施工：把某领取人（或全部）名下已领单全部退回待领
                w = body.get("worker", "")
                freed = []
                for x in tasks:
                    if x["状态"] == "已领" and (w == "" or x["领取人"] == w):
                        freed.append(f"{x['id']}({x['领取人']})")
                        x["状态"], x["领取人"], x["结果"], x["时间"] = "待领", "", "", ""
                if not freed:
                    return self._json({"ok": False, "err": "该领取人名下没有在施工的单"})
                write_back(lines, tasks)
                return self._json({"ok": True, "freed": freed, "n": len(freed)})
            if not t:
                return self._json({"ok": False, "err": "no such id"}, 404)
            if act == "/api/complete":
                if t["状态"] not in ("已领", "阻塞"):
                    return self._json({"ok": False, "err": "状态不允许完成"}, 409)
                if body.get("worker") and body["worker"] != t["领取人"]:
                    return self._json({"ok": False, "err": "非本单领取人"}, 403)
                t["状态"], t["结果"] = "已完成", body.get("result", "")[:200]
                t["时间"] = time.strftime("%m-%d %H:%M")
            elif act == "/api/block":
                t["状态"], t["结果"] = "阻塞", body.get("reason", "")[:200]
                t["时间"] = time.strftime("%m-%d %H:%M")
            elif act == "/api/release":
                t["状态"], t["领取人"], t["结果"], t["时间"] = "待领", "", "", ""
            else:
                return self._json({"ok": False, "err": "unknown action"}, 404)
            write_back(lines, tasks)
        return self._json({"ok": True, "task": {k: v for k, v in t.items() if not k.startswith("_")}})

def open_window():
    """只用独立 App 窗口（Edge --app）打开，不开浏览器标签"""
    url = f"http://127.0.0.1:{PORT}"
    cands = [
        os.path.expandvars(r"%ProgramFiles(x86)%\Microsoft\Edge\Application\msedge.exe"),
        os.path.expandvars(r"%ProgramFiles%\Microsoft\Edge\Application\msedge.exe"),
        os.path.expandvars(r"%LocalAppData%\Google\Chrome\Application\chrome.exe"),
    ]
    for exe in cands:
        if os.path.exists(exe):
            subprocess.Popen([exe, f"--app={url}", "--window-size=1500,940", "--start-fullscreen",
                              "--disable-features=Translate"])
            return
    webbrowser.open(url)  # 兜底

def main():
    if not os.path.exists(MD_PATH):
        sys.exit("taskboard.md 不在程序同目录：" + APP_DIR)
    srv = ThreadingHTTPServer(("127.0.0.1", PORT), H)
    print(f"VTaskBoard on http://127.0.0.1:{PORT}  |  任务册: {MD_PATH}")
    threading.Timer(0.8, open_window).start()
    srv.serve_forever()

if __name__ == "__main__":
    main()
