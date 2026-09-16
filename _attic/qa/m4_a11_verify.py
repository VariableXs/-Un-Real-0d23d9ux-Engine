# -*- coding: utf-8 -*-
# M4-A11: graceful exit (WM_CLOSE to desktop main window) -> process exits ->
# Shell_TrayWnd visible again + workarea restored.
import ctypes, ctypes.wintypes as wt, json, subprocess, time, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from m4_a6_probe import CDP, u32, grab
import m4_probe as mp

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
                cbn = ctypes.create_unicode_buffer(256)
                u32.GetClassNameW(h, cbn, 256)
                if cbn.value == "Tauri Window":
                    out.append(h)
        return True
    u32.EnumWindows(EnumProc(cb), 0)
    return out

def variable_pids():
    r = subprocess.run(["tasklist", "/FO", "CSV"], capture_output=True)
    out = r.stdout.decode("gbk", "replace")
    return [l.split(",")[1].strip('"') for l in out.splitlines() if "variable.exe" in l.lower()]

def tray_state():
    tray = mp.find_by_class("Shell_TrayWnd")
    vis = mp.is_visible(tray) if tray else False
    return {"hwnd": tray, "visible": vis, "rect": mp.rect_of(tray) if tray else None}

def main():
    rep = {}
    rep["pids_before"] = variable_pids()
    rep["tray_before"] = tray_state()

    dw = desktop_hwnd()
    rep["desktop_hwnd"] = dw
    if not dw:
        print(json.dumps(rep)); return
    u32.PostMessageW(dw[0], WM_CLOSE, 0, 0)
    time.sleep(5)
    rep["pids_after"] = variable_pids()
    rep["exited_clean"] = len(rep["pids_after"]) == 0
    time.sleep(2)
    rep["tray_after"] = tray_state()
    grab("_attic/qa/m4_a11_after_exit.png")
    print(json.dumps(rep, ensure_ascii=False, indent=1))

if __name__ == "__main__":
    main()
