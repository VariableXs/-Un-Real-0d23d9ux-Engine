# -*- coding: utf-8 -*-
# M4-A6 exp11 (final diagnosis): unactivated window - where does the click die?
# p1: read exstyle right before/after physical click (trans bit snapshot)
# p2: PostMessage WM_LBUTTONDOWN to OUTER hwnd (Tauri Window)
# p3: PostMessage to WRY_WEBVIEW child
import ctypes, ctypes.wintypes as wt, json, time, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from m4_a6_probe import (CDP, JS_STATE, find_taskbar_hwnds, rect_of,
                         move_cursor, phys_click, cursor_pos, u32, grab)

WM_LBUTTONDOWN = 0x0201
WM_LBUTTONUP   = 0x0202
MK_LBUTTON     = 0x0001

def find_children(tb, cls_sub):
    out = []
    CB = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def cb(h, _):
        cbn = ctypes.create_unicode_buffer(256)
        u32.GetClassNameW(h, cbn, 256)
        if cls_sub in cbn.value:
            out.append(h)
        return True
    u32.EnumChildWindows(tb, CB(cb), 0)
    return out

def post_click(h, x, y):
    lp = (y & 0xFFFF) << 16 | (x & 0xFFFF)
    u32.PostMessageW(h, WM_LBUTTONDOWN, MK_LBUTTON, lp)
    time.sleep(0.07)
    u32.PostMessageW(h, WM_LBUTTONUP, 0, lp)

def main():
    rep = {}
    tb = find_taskbar_hwnds()[0]
    cdp = CDP()
    sw = u32.GetSystemMetrics(0); sh = u32.GetSystemMetrics(1)
    move_cursor(sw // 2, sh - 3)
    time.sleep(0.9)
    st1 = json.loads(cdp.evaluate(JS_STATE))
    dpr = st1["dpr"]
    if st1["btn"][1] > sh / dpr:
        print(json.dumps({"error": "not shown"})); return

    cdp.evaluate(
        "window.__ev3=[];"
        "['pointerdown','click'].forEach(t=>"
        "document.addEventListener(t,e=>window.__ev3.push(t+':'+Math.round(e.clientX)+','+Math.round(e.clientY)+':'+((e.target.className||'').toString().slice(0,20))),true));'ok'")

    def n_ev():
        return int(cdp.evaluate("window.__ev3.length"))

    def close_menu():
        cdp.evaluate(
            "if(document.querySelector('.start-menu'))"
            "{document.querySelector('[aria-label=\"\\u5f00\\u59cb\"]').click();};'ok'")
        time.sleep(0.4)

    EX_T = 0x20
    ex = lambda: u32.GetWindowLongPtrW(tb, -20) & 0xFFFFFFFF

    # p1: exstyle snapshot around physical click (window stays unactivated)
    goto = (564, 834)  # real V center, logic
    cx, cy = int(goto[0] * dpr), int(goto[1] * dpr)
    move_cursor(cx, cy)
    time.sleep(0.6)
    rep["p1"] = {}
    rep["p1"]["ex_before"] = hex(ex())
    base = n_ev()
    phys_click()
    rep["p1"]["ex_after_click"] = hex(ex())
    time.sleep(0.5)
    rep["p1"]["ev_added"] = n_ev() - base
    rep["p1"]["menu"] = json.loads(cdp.evaluate(JS_STATE))["startMenu"]
    close_menu()

    # p2: PostMessage to outer window (client coords = physical here? use client)
    # outer rect is (0,0,1920,1080) physical; client coords in px units of the wnd
    base = n_ev()
    post_click(tb, cx, cy)
    time.sleep(0.5)
    rep["p2_outer"] = {"ev_added": n_ev() - base,
                       "menu": json.loads(cdp.evaluate(JS_STATE))["startMenu"]}
    close_menu()

    # p3: PostMessage to WRY_WEBVIEW child
    wry = find_children(tb, "WRY_WEBVIEW")
    rep["p3_wry"] = {"found": len(wry)}
    if wry:
        base = n_ev()
        post_click(wry[0], cx, cy)
        time.sleep(0.5)
        rep["p3_wry"].update({"ev_added": n_ev() - base,
                              "menu": json.loads(cdp.evaluate(JS_STATE))["startMenu"]})
        close_menu()

    print(json.dumps(rep, ensure_ascii=False, indent=1))

if __name__ == "__main__":
    main()
