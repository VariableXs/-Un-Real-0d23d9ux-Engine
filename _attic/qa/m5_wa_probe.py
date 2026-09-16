# -*- coding: utf-8 -*-
"""工作区探针：SPI_GETWORKAREA（argtypes 显式）+ Shell_TrayWnd 可见性。"""
import ctypes, ctypes.wintypes as wt, json
try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:
    pass
u32 = ctypes.WinDLL("user32", use_last_error=True)
u32.SystemParametersInfoW.argtypes = [wt.UINT, wt.UINT, ctypes.c_void_p, wt.UINT]
u32.SystemParametersInfoW.restype = wt.BOOL
r = wt.RECT()
ok = u32.SystemParametersInfoW(0x0048, 0, ctypes.cast(ctypes.byref(r), ctypes.c_void_p), 0)
tray = u32.FindWindowW("Shell_TrayWnd", None)
tr = wt.RECT()
u32.GetWindowRect(wt.HWND(tray), ctypes.byref(tr))
print(json.dumps({
    "get_ok": bool(ok),
    "workarea": [r.left, r.top, r.right, r.bottom],
    "tray_visible": bool(u32.IsWindowVisible(wt.HWND(tray))),
    "tray_rect": [tr.left, tr.top, tr.right, tr.bottom],
}))
