# -*- coding: utf-8 -*-
# M4-A6 experiment 7: timeline capture.
# A) SetForegroundWindow -> physical click -> 50ms x 20 sampling of menu/active
#    + MutationObserver log of .start-menu show/hide + window focus/blur log
# B) two consecutive physical clicks (no FG reset) -> does click #2 work?
import ctypes, json, time, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from m4_a6_probe import (CDP, JS_STATE, find_taskbar_hwnds, move_cursor,
                         phys_click, cursor_pos, u32, grab)

WATCH_JS = r"""
window.__ev=[];window.__log=[];
const clsOf=e=>{const t=e.target;return (t.className&&t.className.toString?t2s(t.className):t.tagName);};
const t2s=c=>c.toString().slice(0,40);
['pointerdown','mousedown','mouseup','click'].forEach(t=>
  document.addEventListener(t,e=>window.__ev.push({type:t,cls:clsOf(e),y:Math.round(e.clientY)}),true));
window.addEventListener('focus',()=>window.__log.push('WIN-FOCUS@'+Date.now()%100000));
window.addEventListener('blur',()=>window.__log.push('WIN-BLUR@'+Date.now()%100000));
const mo=new MutationObserver(()=>{
  const m=!!document.querySelector('.start-menu');
  const a=!!document.querySelector('.tb-v.active');
  const last=window.__lastMenu;
  if(m!==last){window.__log.push((m?'MENU-OPEN':'MENU-CLOSE')+'@'+Date.now()%100000);window.__lastMenu=m;}
});
mo.observe(document.body,{childList:true,subtree:true,attributes:true});
window.__lastMenu=!!document.querySelector('.start-menu');
'ok'
"""

def sample(cdp, n=20, dt=0.05):
    out = []
    for _ in range(n):
        out.append(cdp.evaluate(
            "JSON.stringify({menu:!!document.querySelector('.start-menu'),"
            "active:!!document.querySelector('.tb-v.active'),"
            "ev:window.__ev.length,log:window.__log.slice(-4)})"))
        time.sleep(dt)
    return out

def goto_btn(cdp):
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

def close_menu_js(cdp):
    cdp.evaluate(
        "if(document.querySelector('.start-menu'))"
        "{document.querySelector('[aria-label=\"\\u5f00\\u59cb\"]').click();};'ok'")
    time.sleep(0.4)

def main():
    rep = {}
    tb = find_taskbar_hwnds()[0]
    cdp = CDP()

    # ---- Stage A: FG + click + timeline ----
    loc = goto_btn(cdp)
    if not loc:
        print(json.dumps({"error": "not shown"})); return
    close_menu_js(cdp)
    cdp.evaluate(WATCH_JS)
    u32.SetForegroundWindow(tb)
    time.sleep(0.3)
    rep["fg_set"] = u32.GetForegroundWindow() == tb
    phys_click()
    rep["A_timeline"] = sample(cdp, 20, 0.05)
    rep["A_fg_after"] = u32.GetForegroundWindow() == tb
    grab("_attic/qa/m4_a6_7A.png")

    # ---- Stage B: close menu if open, then two plain clicks ----
    close_menu_js(cdp)
    cdp.evaluate("window.__ev=[];window.__log=[];'ok'")
    phys_click()
    time.sleep(0.35)
    st_after1 = json.loads(cdp.evaluate(JS_STATE))
    phys_click()
    time.sleep(0.6)
    rep["B_state_after_click1"] = {"menu": st_after1.get("startMenu")}
    rep["B_final"] = cdp.evaluate(
        "JSON.stringify({menu:!!document.querySelector('.start-menu'),"
        "ev:window.__ev,log:window.__log.slice(-8)})")
    grab("_attic/qa/m4_a6_7B.png")
    print(json.dumps(rep, ensure_ascii=False, indent=1))

if __name__ == "__main__":
    main()
