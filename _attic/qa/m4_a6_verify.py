# -*- coding: utf-8 -*-
# M4-A6 FINAL VERIFY: summon -> hover (>=250ms triggers hover-focus) -> physical
# click opens menu; click again closes; backdrop click closes. No manual FG.
import ctypes, json, time, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from m4_a6_probe import (CDP, JS_STATE, find_taskbar_hwnds, move_cursor,
                         phys_click, cursor_pos, u32, grab)

APPLOG = os.path.join(os.environ["APPDATA"], "com.variable.app", "logs")

def tail_applog(n=6):
    try:
        files = sorted(f for f in os.listdir(APPLOG) if f.startswith("applog-"))
        with open(os.path.join(APPLOG, files[-1]), encoding="utf-8", errors="replace") as fh:
            return fh.readlines()[-n:]
    except Exception as e:
        return [f"applog-err: {e}"]

def main():
    rep = {}
    tb = find_taskbar_hwnds()[0]
    cdp = CDP()
    sw = u32.GetSystemMetrics(0); sh = u32.GetSystemMetrics(1)

    def close_menu():
        cdp.evaluate(
            "if(document.querySelector('.start-menu'))"
            "{document.querySelector('[aria-label=\"\\u5f00\\u59cb\"]').click();};'ok'")
        time.sleep(0.4)

    # summon
    move_cursor(sw // 2, sh - 3)
    time.sleep(1.0)
    st1 = json.loads(cdp.evaluate(JS_STATE))
    rep["summoned_shown"] = "tbw-hidden" not in (st1["rootClass"] or "")

    # hover REAL V center (564, 834 logic) - hold > HOVER_FOCUS_MS
    move_cursor(int(564 * 1.25), int(834 * 1.25))
    time.sleep(0.9)  # watcher cycles: hover timer 250ms + set_focus
    rep["fg_before_click"] = u32.GetForegroundWindow() == tb

    # physical click -> menu should OPEN
    phys_click()
    time.sleep(0.6)
    st2 = json.loads(cdp.evaluate(JS_STATE))
    rep["click1_menu_opened"] = st2["startMenu"]
    grab("_attic/qa/m4_a6_V1_menu_open.png")

    # physical click again -> menu should CLOSE (toggle)
    phys_click()
    time.sleep(0.6)
    st3 = json.loads(cdp.evaluate(JS_STATE))
    rep["click2_menu_closed"] = not st3["startMenu"]

    # open again, then backdrop click (menu area outside) -> close
    phys_click()
    time.sleep(0.6)
    st4 = json.loads(cdp.evaluate(JS_STATE))
    rep["click3_reopened"] = st4["startMenu"]
    if st4["startMenu"]:
        move_cursor(int(564 * 1.25), int(400 * 1.25))  # above taskbar, menu backdrop zone
        time.sleep(0.4)
        phys_click()
        time.sleep(0.6)
        st5 = json.loads(cdp.evaluate(JS_STATE))
        rep["backdrop_click_closed"] = not st5["startMenu"]
        # move cursor away -> taskbar should collapse again
        move_cursor(sw // 2, sh // 2)
        time.sleep(2.0)
        st6 = json.loads(cdp.evaluate(JS_STATE))
        rep["away_collapsed"] = "tbw-hidden" in (st6["rootClass"] or "")
    rep["applog_tail"] = [l.strip() for l in tail_applog(8)]
    print(json.dumps(rep, ensure_ascii=False, indent=1))

if __name__ == "__main__":
    main()
