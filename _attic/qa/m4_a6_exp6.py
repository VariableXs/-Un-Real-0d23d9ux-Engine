# -*- coding: utf-8 -*-
# M4-A6 experiment 6: WHO receives the click & does the menu flash open?
# - elementFromPoint chain at click point
# - record event target className per event
# - sample menu state at 100/300/700ms after the physical click
import ctypes, json, time, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from m4_a6_probe import (CDP, JS_STATE, find_taskbar_hwnds, move_cursor,
                         phys_click, cursor_pos, u32, grab)

def main():
    rep = {}
    cdp = CDP()
    sw = u32.GetSystemMetrics(0); sh = u32.GetSystemMetrics(1)
    move_cursor(sw // 2, sh - 3)
    time.sleep(0.9)
    st1 = json.loads(cdp.evaluate(JS_STATE))
    dpr = st1["dpr"]
    if st1["btn"][1] > sh / dpr:
        print(json.dumps({"error": "not shown", "state": st1})); return
    bx, by, bw, bh = st1["btn"]
    cx, cy = int((bx + bw / 2) * dpr), int((by + bh / 2) * dpr)
    move_cursor(cx, cy)
    time.sleep(0.5)

    # pre-click DOM ground truth
    rep["pre"] = cdp.evaluate(r"""JSON.stringify((() => {
      const el = document.elementFromPoint(768, 833);
      const chain = [];
      let n = el;
      while (n && chain.length < 6) { chain.push(n.tagName + '.' + (n.className && n.className.toString ? n.className.toString().slice(0,60) : '')); n = n.parentElement; }
      const btn = document.querySelector('[aria-label="\u5f00\u59cb"]');
      const bs = btn ? getComputedStyle(btn) : null;
      return {
        chain,
        btnDisabled: btn ? btn.disabled : null,
        btnPE: bs ? bs.pointerEvents : null,
        btnRect: btn ? (r => [r.x, r.y, r.width, r.height])(btn.getBoundingClientRect()) : null,
        rootClass: document.querySelector('[data-testid="taskbar-window-root"]').className
      };
})())""")

    cdp.evaluate(
        "window.__ev=[];"
        "['mousedown','pointerdown','click'].forEach(t=>"
        "document.addEventListener(t,e=>{const t2=e.target;"
        "window.__ev.push({type:t,cls:(t2.className&&t2.className.toString?t2.className.toString().slice(0,50):t2.tagName),"
        "path:e.composedPath().slice(0,3).map(n=>n.tagName+'.'+(n.className&&n.className.toString?n.className.toString().slice(0,40):''))});},true));"
        "'ok'")

    phys_click()
    # sample menu state quickly
    samples = []
    for delay in (0.1, 0.3, 0.7):
        time.sleep(delay)
        samples.append(cdp.evaluate(
            "JSON.stringify({t:Date.now()%100000,menu:!!document.querySelector('.start-menu'),"
            "rootClass:document.querySelector('[data-testid=\"taskbar-window-root\"]').className,"
            "evN:window.__ev.length})"))
    rep["samples"] = samples
    rep["events"] = cdp.evaluate("JSON.stringify(window.__ev)")
    grab("_attic/qa/m4_a6_6_final.png")
    print(json.dumps(rep, ensure_ascii=False, indent=1))

if __name__ == "__main__":
    main()
