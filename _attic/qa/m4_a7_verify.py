# -*- coding: utf-8 -*-
# M4-A7: close notepad (adopted) -> taskbar must REAPPEAR (embedded=0 -> shown).
import ctypes, ctypes.wintypes as wt, json, time, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from m4_a6_probe import CDP, JS_STATE, move_cursor, u32, grab

WM_CLOSE = 0x0010

def find_notepad_windows():
    out = []
    EnumProc = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def cb(h, _):
        cbn = ctypes.create_unicode_buffer(256)
        u32.GetClassNameW(h, cbn, 256)
        if cbn.value == "Notepad":
            n = u32.GetWindowTextLengthW(h)
            b = ctypes.create_unicode_buffer(n + 1) if n else None
            if b: u32.GetWindowTextW(h, b, n + 1)
            out.append({"hwnd": h, "title": b.value if b else ""})
        return True
    u32.EnumWindows(EnumProc(cb), 0)
    return out

def main():
    rep = {}
    cdp = CDP()
    sw = u32.GetSystemMetrics(0); sh = u32.GetSystemMetrics(1)
    # keep cursor away from bottom so state is governed by embedded count only
    move_cursor(sw // 2, sh // 3)
    time.sleep(0.5)

    rep["before"] = {"notepads": find_notepad_windows(),
                     "state": json.loads(cdp.evaluate(JS_STATE))["rootClass"]}

    for np in rep["before"]["notepads"]:
        u32.PostMessageW(np["hwnd"], WM_CLOSE, 0, 0)
    time.sleep(2.5)

    st = json.loads(cdp.evaluate(JS_STATE))
    rep["after_close"] = {"notepads": find_notepad_windows(),
                          "rootClass": st["rootClass"],
                          "startMenu": st["startMenu"]}
    grab("_attic/qa/m4_a7_reappear.png")
    print(json.dumps(rep, ensure_ascii=False, indent=1))

if __name__ == "__main__":
    main()
