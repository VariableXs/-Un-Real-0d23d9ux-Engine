# -*- coding: utf-8 -*-
"""工作区行为验证：开 notepad → 最大化 → rect 底边 = 工作区底。"""
import ctypes, ctypes.wintypes as wt, subprocess, time, json
u32 = ctypes.WinDLL("user32", use_last_error=True)
subprocess.run(["taskkill", "/F", "/IM", "notepad.exe"], capture_output=True)
time.sleep(1)
subprocess.Popen(["notepad.exe"])
time.sleep(2)
hwnds = []
EnumProc = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
def cb(h, _):
    n = u32.GetWindowTextLengthW(h)
    if n and u32.IsWindowVisible(h):
        b = ctypes.create_unicode_buffer(n + 1)
        u32.GetWindowTextW(h, b, n + 1)
        if "Notepad" in b.value or "记事本" in b.value:
            hwnds.append(h)
    return True
u32.EnumWindows(EnumProc(cb), 0)
rep = {"hwnds": hwnds}
if hwnds:
    u32.ShowWindow(hwnds[0], 3)  # SW_MAXIMIZE
    time.sleep(1)
    rc = wt.RECT()
    u32.GetWindowRect(hwnds[0], ctypes.byref(rc))
    rep["maximized_rect"] = [rc.left, rc.top, rc.right, rc.bottom]
    rep["workarea_bottom_inferred"] = rc.bottom  # 1020=有任务栏区 / 1080=全屏
    u32.PostMessageW(hwnds[0], 0x0010, 0, 0)  # WM_CLOSE
print(json.dumps(rep))
