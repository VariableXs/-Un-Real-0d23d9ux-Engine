# -*- coding: utf-8 -*-
# M4-A6 exp10: activation <-> click reachability matrix.
# s1: no activation, click real V -> counters?
# s2: SetForegroundWindow, click real V -> counters + menu?
# s3: click explorer btn (opens app -> blur), then click real V -> counters + menu?
import ctypes, json, time, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from m4_a6_probe import (CDP, JS_STATE, find_taskbar_hwnds, move_cursor,
                         phys_click, cursor_pos, u32, grab)

TB = 9223  # cdp port

def goto(cdp, x_logic, y_logic, dpr):
    cx, cy = int(x_logic * dpr), int(y_logic * dpr)
    move_cursor(cx, cy)
    time.sleep(0.5)
    return cx, cy

def main():
    rep = {}
    tb = find_taskbar_hwnds()[0]
    cdp = CDP()
    sw = u32.GetSystemMetrics(0); sh = u32.GetSystemMetrics(1)
    move_cursor(sw // 2, sh - 3)
    time.sleep(0.9)
    st1 = json.loads(cdp.evaluate(JS_STATE))
    dpr = st1["dpr"]

    # inject counters once (idempotent across stages, we snapshot len)
    cdp.evaluate(
        "window.__ev2=[];"
        "['pointerdown','click'].forEach(t=>"
        "document.addEventListener(t,e=>window.__ev2.push(t+':'+Math.round(e.clientX)+','+Math.round(e.clientY)+':'+((e.target.className||'').toString().slice(0,20))),true));'ok'")

    def n_ev():
        return cdp.evaluate("window.__ev2.length")

    def close_menu():
        cdp.evaluate(
            "if(document.querySelector('.start-menu'))"
            "{document.querySelector('[aria-label=\"\\u5f00\\u59cb\"]').click();};'ok'")
        time.sleep(0.4)

    # s1: NO activation, click real V (544..584 center=564, y=834)
    rep["s1_fg_before"] = u32.GetForegroundWindow() == tb
    goto(cdp, 564, 834, dpr)
    base = int(n_ev())
    phys_click()
    time.sleep(0.6)
    rep["s1"] = {"ev_added": int(n_ev()) - base,
                 "menu": json.loads(cdp.evaluate(JS_STATE))["startMenu"],
                 "fg_after": u32.GetForegroundWindow() == tb}
    close_menu()

    # s2: activate, click real V
    u32.SetForegroundWindow(tb)
    time.sleep(0.3)
    rep["s2_fg_before"] = u32.GetForegroundWindow() == tb
    base = int(n_ev())
    phys_click()
    time.sleep(0.6)
    rep["s2"] = {"ev_added": int(n_ev()) - base,
                 "menu": json.loads(cdp.evaluate(JS_STATE))["startMenu"],
                 "fg_after": u32.GetForegroundWindow() == tb}
    grab("_attic/qa/m4_a6_10_s2.png")
    close_menu()

    # s3: activate, click explorer (645+20=665 -> opens app), then click V
    u32.SetForegroundWindow(tb)
    time.sleep(0.3)
    goto(cdp, 665, 834, dpr)
    base = int(n_ev())
    phys_click()
    time.sleep(0.8)
    mid = int(n_ev()) - base
    goto(cdp, 564, 834, dpr)
    base = int(n_ev())
    phys_click()
    time.sleep(0.6)
    rep["s3"] = {"explorer_ev": mid,
                 "v_ev": int(n_ev()) - base,
                 "menu": json.loads(cdp.evaluate(JS_STATE))["startMenu"],
                 "fg_after_v_click": u32.GetForegroundWindow() == tb}
    grab("_attic/qa/m4_a6_10_s3.png")

    rep["events_tail"] = cdp.evaluate("JSON.stringify(window.__ev2.slice(-8))")
    print(json.dumps(rep, ensure_ascii=False, indent=1))

if __name__ == "__main__":
    main()
