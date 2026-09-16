# -*- coding: utf-8 -*-
# M4-A6 experiment 4: who receives the physical click at the button point?
# - WindowFromPoint / ChildWindowFromPoint chain at cursor
# - DOM mousedown/pointerdown counters via CDP
# - one physical click, then read counters
import ctypes, ctypes.wintypes as wt, json, time, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from m4_a6_probe import (CDP, JS_STATE, exstyle, find_taskbar_hwnds, rect_of,
                         move_cursor, phys_click, cursor_pos, grab, u32)

def who_at(x, y):
    pt = wt.POINT(x, y)
    top = u32.WindowFromPoint(pt)
    out = {"top_hwnd": top, "top_title": "", "top_class": ""}
    if top:
        n = u32.GetWindowTextLengthW(top)
        b = ctypes.create_unicode_buffer(n + 1) if n else None
        if b: u32.GetWindowTextW(top, b, n + 1)
        cb = ctypes.create_unicode_buffer(256)
        u32.GetClassNameW(top, cb, 256)
        out["top_title"] = b.value if b else ""
        out["top_class"] = cb.value
    return out

def enum_zorder():
    out = []
    EnumProc = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def cb(h, _):
        if u32.IsWindowVisible(h):
            n = u32.GetWindowTextLengthW(h)
            b = ctypes.create_unicode_buffer(n + 1) if n else ctypes.create_unicode_buffer(1)
            if n: u32.GetWindowTextW(h, b, n + 1)
            cbn = ctypes.create_unicode_buffer(256)
            u32.GetClassNameW(h, cbn, 256)
            rc = rect_of(h)
            out.append({"hwnd": h, "title": b.value[:40], "cls": cbn.value[:40],
                        "rect": rc, "ex": hex(exstyle(h))})
        return len(out) < 25  # top 25 only
    u32.EnumWindows(EnumProc(cb), 0)
    return out

def main():
    rep = {}
    tb = find_taskbar_hwnds()[0]
    rep["tb_hwnd"] = tb

    # summon first (cursor at hotzone), then hover button
    sw = u32.GetSystemMetrics(0); sh = u32.GetSystemMetrics(1)
    move_cursor(sw // 2, sh - 3)
    time.sleep(0.9)

    cdp = CDP()
    st1 = json.loads(cdp.evaluate(JS_STATE))
    dpr = st1["dpr"]
    if st1["btn"][1] > sh / dpr:
        print(json.dumps({"error": "not shown", "state": st1})); return
    bx, by, bw, bh = st1["btn"]
    cx, cy = int((bx + bw / 2) * dpr), int((by + bh / 2) * dpr)
    move_cursor(cx, cy)
    time.sleep(0.5)

    # DOM counters
    cdp.evaluate(
        "window.__md=0;window.__pd=0;window.__mu=0;"
        "document.addEventListener('mousedown',e=>{window.__md++;window.__mdx=e.clientX;window.__mdy=e.clientY;},true);"
        "document.addEventListener('pointerdown',e=>{window.__pd++;},true);"
        "document.addEventListener('mouseup',e=>{window.__mu++;},true);'ok'")

    rep["cursor"] = list(cursor_pos())
    rep["who_at_btn"] = who_at(cx, cy)
    rep["zorder_top"] = enum_zorder()
    rep["tb_visible"] = bool(u32.IsWindowVisible(tb))
    rep["tb_rect"] = rect_of(tb)

    phys_click()
    time.sleep(0.6)

    rep["counters"] = cdp.evaluate("JSON.stringify({md:window.__md,mdx:window.__mdx,mdy:window.__mdy,pd:window.__pd,mu:window.__mu})")
    rep["after_state"] = json.loads(cdp.evaluate(JS_STATE))
    grab("_attic/qa/m4_a6_4_after_phys.png")
    print(json.dumps(rep, ensure_ascii=False, indent=1))

if __name__ == "__main__":
    main()
