# -*- coding: utf-8 -*-
"""A11 同款：只对 desktop 主窗（Private Desktop Environment + Tauri Window）发 WM_CLOSE。"""
import ctypes, ctypes.wintypes as wt, json, time
u32 = ctypes.WinDLL("user32", use_last_error=True)
WM_CLOSE = 0x0010

def desktop_hwnd():
    out = []
    EnumProc = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def cb(h, _):
        n = u32.GetWindowTextLengthW(h)
        if n:
            b = ctypes.create_unicode_buffer(n + 1)
            u32.GetWindowTextW(h, b, n + 1)
            if "Private Desktop Environment" in b.value and u32.IsWindowVisible(h):
                cn = ctypes.create_unicode_buffer(256)
                u32.GetClassNameW(h, cn, 256)
                if cn.value == "Tauri Window":
                    out.append(h)
        return True
    u32.EnumWindows(EnumProc(cb), 0)
    return out

dw = desktop_hwnd()
print("desktop hwnds:", dw)
for h in dw:
    u32.PostMessageW(h, WM_CLOSE, 0, 0)
print("posted")
