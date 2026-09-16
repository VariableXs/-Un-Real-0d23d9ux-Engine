import ctypes, ctypes.wintypes as wt, json
try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:
    pass
u32 = ctypes.WinDLL("user32", use_last_error=True)
r = wt.RECT()
ok = u32.SystemParametersInfoW(0x0048, 0, ctypes.byref(r), 0)
tray = u32.FindWindowW("Shell_TrayWnd", None)
print(json.dumps({"ok": bool(ok), "wa": (r.left, r.top, r.right, r.bottom),
                  "tray_visible": bool(u32.IsWindowVisible(wt.HWND(tray))),
                  "tray_rect": None}))
r2 = wt.RECT()
u32.GetWindowRect(wt.HWND(tray), ctypes.byref(r2))
print("tray rect:", (r2.left, r2.top, r2.right, r2.bottom))
