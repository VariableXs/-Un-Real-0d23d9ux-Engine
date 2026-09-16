# -*- coding: utf-8 -*-
# M4-A10: full lifecycle loop in one pass.
# adopt -> collapsed -> summon -> physical click opens menu -> close ->
# kill 3rd party -> taskbar reappears. Every checkpoint asserted.
import ctypes, ctypes.wintypes as wt, json, subprocess, time, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from m4_a6_probe import CDP, JS_STATE, move_cursor, phys_click, u32, grab

def notepad_hwnds():
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
    rep, ok = {}, True
    cdp = CDP()
    sw = u32.GetSystemMetrics(0); sh = u32.GetSystemMetrics(1)

    def chk(name, cond):
        nonlocal ok
        rep[name] = bool(cond)
        ok = ok and bool(cond)

    # 1. launch + adopt
    subprocess.Popen(["notepad.exe"])
    time.sleep(10)
    chk("adopted_notepad_visible", len(notepad_hwnds()) > 0)
    move_cursor(sw // 2, sh // 3); time.sleep(1.5)
    st = json.loads(cdp.evaluate(JS_STATE))
    chk("collapsed_after_adopt", "tbw-hidden" in st["rootClass"])

    # 2. summon
    move_cursor(sw // 2, sh - 3); time.sleep(1.2)
    st = json.loads(cdp.evaluate(JS_STATE))
    chk("summoned", "tbw-hidden" not in st["rootClass"])

    # 3. physical click opens menu（动态定位真按钮：改版后硬坐标落在容器 DIV 上）
    dpr = st.get("dpr") or 1.25
    loc = json.loads(cdp.evaluate('''(function(){
        const b = document.querySelector('button[aria-label="\\u5f00\\u59cb"]');
        if (!b) return JSON.stringify({err: 1});
        const r = b.getBoundingClientRect();
        return JSON.stringify({x: r.x + r.width / 2, y: r.y + r.height / 2});
    })()'''))
    if "err" in loc:
        print(json.dumps({"fatal": "start button not found"})); return
    move_cursor(int(loc["x"] * dpr), int(loc["y"] * dpr)); time.sleep(0.9)
    phys_click(); time.sleep(0.6)
    st = json.loads(cdp.evaluate(JS_STATE))
    chk("menu_opened_by_click", st["startMenu"])
    grab("_attic/qa/m4_a10_1_menu.png")

    # 4. close via V toggle
    phys_click(); time.sleep(0.6)
    st = json.loads(cdp.evaluate(JS_STATE))
    chk("menu_closed_by_click", not st["startMenu"])

    # 5. kill 3rd party -> reappear
    subprocess.run(["taskkill", "/F", "/IM", "Notepad.exe"], capture_output=True)
    time.sleep(3)
    move_cursor(sw // 2, sh // 3); time.sleep(2)
    st = json.loads(cdp.evaluate(JS_STATE))
    chk("reappeared_after_exit", "tbw-hidden" not in st["rootClass"])
    grab("_attic/qa/m4_a10_2_reappear.png")

    rep["ALL_PASS"] = ok
    print(json.dumps(rep, ensure_ascii=False, indent=1))

if __name__ == "__main__":
    main()
