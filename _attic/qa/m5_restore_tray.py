# -*- coding: utf-8 -*-
"""强杀 Variable 后手动恢复 Windows 任务栏 + 工作区（等效 taskbar_win::restore_windows_traces）。"""
import ctypes, ctypes.wintypes as wt, json
try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:
    pass
u32 = ctypes.WinDLL("user32", use_last_error=True)
SPI_SETWORKAREA = 0x00  # placeholder overwritten below
SPI_SETWORKAREA = 0x002F
SPI_GETWORKAREA = 0x0048

def find_window(cls):
    return u32.FindWindowW(cls, None) or 0

def rect_of(h):
    r = wt.RECT()
    u32.GetWindowRect(wt.HWND(h), ctypes.byref(r))
    return (r.left, r.top, r.right, r.bottom)

rep = {}
tray = find_window("Shell_TrayWnd")
sec = 0
EnumProc = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
secs = []
def cb(h, _):
    cn = ctypes.create_unicode_buffer(256)
    u32.GetClassNameW(h, cn, 256)
    if cn.value == "Shell_SecondaryTrayWnd":
        secs.append(h)
    return True
u32.EnumWindows(EnumProc(cb), 0)
rep["tray_hwnd"] = tray
rep["tray_rect_before"] = rect_of(tray) if tray else None
shown = 0
if tray and not u32.IsWindowVisible(wt.HWND(tray)):
    u32.ShowWindow(wt.HWND(tray), 5)  # SW_SHOW
    shown += 1
for h in secs:
    if not u32.IsWindowVisible(wt.HWND(h)):
        u32.ShowWindow(wt.HWND(h), 5)
        shown += 1
rep["shown"] = shown
# 工作区 = 全屏减任务栏（主屏底部）
tr = rect_of(tray) if tray else None
if tr:
    wa = wt.RECT(0, 0, tr[2], tr[1])
    ok = u32.SystemParametersInfoW(SPI_SETWORKAREA, 0, ctypes.byref(wa), 0)
    rep["workarea_set"] = (wa.left, wa.top, wa.right, wa.bottom)
    rep["workarea_ok"] = bool(ok)
cur = wt.RECT()
u32.SystemParametersInfoW(SPI_GETWORKAREA, 0, ctypes.byref(cur), 0)
rep["workarea_now"] = (cur.left, cur.top, cur.right, cur.bottom)
rep["tray_visible"] = bool(u32.IsWindowVisible(wt.HWND(tray))) if tray else False
print(json.dumps(rep, ensure_ascii=False))
