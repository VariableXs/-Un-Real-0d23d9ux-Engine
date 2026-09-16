# -*- coding: utf-8 -*-
# M4-A8: maximize adopted notepad -> TRUE fullscreen (workarea is full screen).
# Then hotzone-summon taskbar -> TOPMOST above the maximized window.
import ctypes, ctypes.wintypes as wt, json, subprocess, time, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from m4_a6_probe import CDP, JS_STATE, move_cursor, phys_click, u32, grab

SW_MAXIMIZE = 3

def notepad_hwnd():
    out = []
    EnumProc = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def cb(h, _):
        cbn = ctypes.create_unicode_buffer(256)
        u32.GetClassNameW(h, cbn, 256)
        if cbn.value == "Notepad" and u32.IsWindowVisible(h):
            out.append(h)
        return True
    u32.EnumWindows(EnumProc(cb), 0)
    return out

def main():
    rep = {}
    cdp = CDP()
    sw = u32.GetSystemMetrics(0); sh = u32.GetSystemMetrics(1)
    rep["screen"] = [sw, sh]

    subprocess.Popen(["notepad.exe"])
    time.sleep(4)
    hwnds = notepad_hwnd()
    rep["notepad_found"] = len(hwnds)
    if not hwnds:
        print(json.dumps(rep)); return
    h = hwnds[0]

    # wait for adoption (applog watchdog)
    time.sleep(4)
    u32.ShowWindow(h, SW_MAXIMIZE)
    time.sleep(1.5)

    rc = wt.RECT()
    u32.GetWindowRect(h, ctypes.byref(rc))
    rep["max_rect"] = [rc.left, rc.top, rc.right, rc.bottom]
    rep["true_fullscreen"] = (rc.left, rc.top, rc.right, rc.bottom) == (0, 0, sw, sh)

    st = json.loads(cdp.evaluate(JS_STATE))
    rep["tb_collapsed_with_embedded"] = "tbw-hidden" in st["rootClass"]

    # hotzone summon -> taskbar must appear ABOVE maximized notepad
    move_cursor(sw // 2, sh - 3)
    time.sleep(1.2)
    st2 = json.loads(cdp.evaluate(JS_STATE))
    rep["summoned_over_maximized"] = "tbw-hidden" not in st2["rootClass"]
    grab("_attic/qa/m4_a8_summon_over_max.png")

    # restore cursor, collapse again
    move_cursor(sw // 2, sh // 2)
    time.sleep(2.0)
    print(json.dumps(rep, ensure_ascii=False, indent=1))

if __name__ == "__main__":
    main()
