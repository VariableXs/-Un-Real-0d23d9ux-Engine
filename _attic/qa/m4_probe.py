# M4 trace probe: verify Windows-trace hiding + Variable taskbar window state.
# Pure ctypes, read-only. Usage: python m4_probe.py
import ctypes, ctypes.wintypes as wt, json

try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:
    ctypes.windll.user32.SetProcessDPIAware()
u32 = ctypes.windll.user32
SPI_GETWORKAREA = 0x0048
u32.SystemParametersInfoW.argtypes = [wt.UINT, wt.UINT, ctypes.c_void_p, wt.UINT]
u32.SystemParametersInfoW.restype = wt.BOOL

def find_by_class(cls):
    return u32.FindWindowW(cls, None) or 0

def is_visible(h):
    return bool(u32.IsWindowVisible(h)) if h else False

def rect_of(h):
    rc = wt.RECT()
    if not u32.GetWindowRect(h, ctypes.byref(rc)):
        return None
    return [rc.left, rc.top, rc.right, rc.bottom]

GWL_EXSTYLE = -20
WS_EX_TOPMOST = 0x8
WS_EX_TOOLWINDOW = 0x80
WS_EX_APPWINDOW = 0x40000

def exstyle(h):
    return u32.GetWindowLongPtrW(h, GWL_EXSTYLE) if hasattr(u32, "GetWindowLongPtrW") else u32.GetWindowLongW(h, GWL_EXSTYLE)

def find_by_title_contains(t):
    out = []
    EnumProc = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def cb(h, _):
        n = u32.GetWindowTextLengthW(h)
        if n:
            b = ctypes.create_unicode_buffer(n + 1)
            u32.GetWindowTextW(h, b, n + 1)
            if t.lower() in b.value.lower():
                out.append((h, b.value))
        return True
    u32.EnumWindows(EnumProc(cb), 0)
    return out

report = {}
sw, sh = u32.GetSystemMetrics(0), u32.GetSystemMetrics(1)
report["screen"] = [sw, sh]

tray = find_by_class("Shell_TrayWnd")
report["shell_tray"] = {"hwnd": tray, "visible": is_visible(tray), "rect": rect_of(tray) if tray else None}
sec = []
h = 0
while True:
    h = u32.FindWindowExW(None, h, "Shell_SecondaryTrayWnd", None) or 0
    if not h:
        break
    sec.append({"hwnd": h, "visible": is_visible(h)})
report["shell_secondary"] = sec

wa = wt.RECT()
ok = u32.SystemParametersInfoW(SPI_GETWORKAREA, 0, ctypes.byref(wa), 0)
report["spi_get_returned"] = bool(ok)
report["workarea"] = [wa.left, wa.top, wa.right, wa.bottom]
report["workarea_fullscreen"] = (wa.left == 0 and wa.top == 0 and wa.right == sw and wa.bottom == sh)

tb = find_by_title_contains("Variable Taskbar")
items = []
for h, title in tb:
    vis = is_visible(h)
    if not vis:
        continue  # 只报可见态的（收起态由光标守护管理）
    items.append({
        "hwnd": h, "title": title, "visible": vis, "rect": rect_of(h),
        "topmost": bool(exstyle(h) & WS_EX_TOPMOST),
        "toolwindow": bool(exstyle(h) & WS_EX_TOOLWINDOW),
        "appwindow": bool(exstyle(h) & WS_EX_APPWINDOW),
    })
report["variable_taskbar_visible"] = items

# 收起态窗存在性（不可见也报——证明窗对象活着，只是 SW_HIDE）
report["variable_taskbar_exists_hidden"] = [
    {"hwnd": h, "visible": is_visible(h), "rect": rect_of(h),
     "topmost": bool(exstyle(h) & WS_EX_TOPMOST),
     "toolwindow": bool(exstyle(h) & WS_EX_TOOLWINDOW)}
    for h, _ in tb
]

desk = find_by_title_contains("Variable")
report["variable_windows"] = [{"hwnd": h, "title": t, "visible": is_visible(h), "rect": rect_of(h)} for h, t in desk]

print(json.dumps(report, indent=1, ensure_ascii=False))
