import ctypes, ctypes.wintypes as wt, json
try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:
    pass
u32 = ctypes.WinDLL("user32", use_last_error=True)
SPI_GETWORKAREA = 0x0048
proto = ctypes.WINFUNCTYPE(wt.BOOL, wt.UINT, wt.UINT, wt.LPVOID, wt.UINT)
spi = proto(("SystemParametersInfoW", u32))
r = wt.RECT()
ok = spi(SPI_GETWORKAREA, 0, ctypes.cast(ctypes.byref(r), wt.LPVOID), 0)
print(json.dumps({"ok": bool(ok), "wa": (r.left, r.top, r.right, r.bottom), "err": ctypes.get_last_error()}))
