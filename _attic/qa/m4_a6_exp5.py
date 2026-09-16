# -*- coding: utf-8 -*-
# M4-A6 experiment 5: isolate where the physical click dies.
# g) PostMessage WM_LBUTTONDOWN/UP directly to Chrome input host hwnd
# h) SetForegroundWindow(taskbar) then physical click
# i) IsWindowEnabled / IsIconic checks
import ctypes, ctypes.wintypes as wt, json, time, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from m4_a6_probe import (CDP, JS_STATE, exstyle, find_taskbar_hwnds, rect_of,
                         move_cursor, phys_click, cursor_pos, grab, u32)

WM_LBUTTONDOWN = 0x0201
WM_LBUTTONUP   = 0x0202
MK_LBUTTON     = 0x0001

def find_chrome_host(tb):
    """EnumChildWindows of taskbar -> find Chrome_RenderWidgetHostHWND."""
    out = []
    CB = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def cb(h, _):
        cbn = ctypes.create_unicode_buffer(256)
        u32.GetClassNameW(h, cbn, 256)
        out.append({"hwnd": h, "cls": cbn.value, "visible": bool(u32.IsWindowVisible(h)),
                    "enabled": bool(u32.IsWindowEnabled(h)), "rect": rect_of(h)})
        return True
    u32.EnumChildWindows(tb, CB(cb), 0)
    return out

def phys_click_at_btn(cdp):
    """summon -> locate btn -> move cursor -> return (cdp, cx, cy, chrome_host)."""
    sw = u32.GetSystemMetrics(0); sh = u32.GetSystemMetrics(1)
    move_cursor(sw // 2, sh - 3)
    time.sleep(0.9)
    st1 = json.loads(cdp.evaluate(JS_STATE))
    dpr = st1["dpr"]
    if st1["btn"][1] > sh / dpr:
        return None
    bx, by, bw, bh = st1["btn"]
    cx, cy = int((bx + bw / 2) * dpr), int((by + bh / 2) * dpr)
    move_cursor(cx, cy)
    time.sleep(0.5)
    return cx, cy

def inject_counters(cdp):
    cdp.evaluate(
        "window.__ev=[];"
        "['mousedown','pointerdown','mouseup','click'].forEach(t=>"
        "document.addEventListener(t,e=>window.__ev.push(t+':'+Math.round(e.clientX)+','+Math.round(e.clientY)),true));"
        "'ok'")

def read_counters(cdp):
    return cdp.evaluate("JSON.stringify({ev:window.__ev,menu:!!document.querySelector('.start-menu')})")

def main():
    rep = {}
    tb = find_taskbar_hwnds()[0]
    rep["tb"] = tb
    rep["tb_enabled"] = bool(u32.IsWindowEnabled(tb))
    rep["tb_minimized"] = bool(u32.IsIconic(tb))
    rep["children"] = find_chrome_host(tb)

    cdp = CDP()
    loc = phys_click_at_btn(cdp)
    if not loc:
        print(json.dumps({"error": "not shown"})); return
    cx, cy = loc
    rep["cursor"] = list(cursor_pos())

    # --- g) PostMessage directly to Chrome input host ---
    inject_counters(cdp)
    hosts = [c for c in rep["children"] if "Chrome_RenderWidgetHostHWND" in c["cls"]]
    if hosts:
        host = hosts[0]["hwnd"]
        lp = (cy - 0) << 16 | (cx - 0)  # screen coords not client; host rect is fullscreen so same
        u32.PostMessageW(host, WM_LBUTTONDOWN, MK_LBUTTON, lp)
        time.sleep(0.08)
        u32.PostMessageW(host, WM_LBUTTONUP, 0, lp)
        time.sleep(0.5)
        rep["g_postmessage"] = {"host": host, "result": read_counters(cdp)}

    # close menu if opened by g
    st = json.loads(cdp.evaluate(JS_STATE))
    if st.get("startMenu"):
        cdp.evaluate("document.querySelector('[aria-label=\"\\u5f00\\u59cb\"]').click(); 'ok'")
        time.sleep(0.4)

    # --- h) SetForegroundWindow + physical click ---
    inject_counters(cdp)
    u32.SetForegroundWindow(tb)
    time.sleep(0.3)
    rep["h_fg_set"] = u32.GetForegroundWindow() == tb
    phys_click()
    time.sleep(0.6)
    rep["h_fg_click"] = {"fg_now": u32.GetForegroundWindow() == tb,
                         "result": read_counters(cdp)}

    st = json.loads(cdp.evaluate(JS_STATE))
    if st.get("startMenu"):
        cdp.evaluate("document.querySelector('[aria-label=\"\\u5f00\\u59cb\"]').click(); 'ok'")
        time.sleep(0.4)

    # --- j) baseline: physical click while cursor hovering (no FG change) ---
    inject_counters(cdp)
    phys_click()
    time.sleep(0.6)
    rep["j_plain_click"] = read_counters(cdp)

    print(json.dumps(rep, ensure_ascii=False, indent=1))

if __name__ == "__main__":
    main()
