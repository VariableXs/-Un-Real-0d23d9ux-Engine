# -*- coding: utf-8 -*-
# M4-A6 FINAL: click the REAL visible start button center -> menu opens?
# + list every element whose aria-label contains the start label.
import ctypes, json, time, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from m4_a6_probe import CDP, JS_STATE, move_cursor, phys_click, cursor_pos, u32, grab

JS_DUP = r"""JSON.stringify((() => {
  const out = [];
  document.querySelectorAll('[aria-label]').forEach(e => {
    const lb = e.getAttribute('aria-label');
    if (lb && lb.indexOf('\u5f00\u59cb') >= 0) {
      const r = e.getBoundingClientRect();
      let chain = [], n = e;
      while (n && chain.length < 4) { chain.push(n.tagName + '.' + ((n.className||'').toString().slice(0,30))); n = n.parentElement; }
      const cs = getComputedStyle(e);
      out.push({tag: e.tagName, cls: (e.className||'').toString().slice(0,40),
                rect: [Math.round(r.x), Math.round(r.y), Math.round(r.width), Math.round(r.height)],
                opacity: cs.opacity, display: cs.display, chain});
    }
  });
  return out;
})())"""

def main():
    cdp = CDP()
    sw = u32.GetSystemMetrics(0); sh = u32.GetSystemMetrics(1)
    move_cursor(sw // 2, sh - 3)
    time.sleep(0.9)
    st1 = json.loads(cdp.evaluate(JS_STATE))
    dpr = st1["dpr"]

    rep = {"dup_labels": json.loads(cdp.evaluate(JS_DUP))}

    # real visible start button: aria-label=开始 with width ~40 in the taskbar row
    real = None
    for e in rep["dup_labels"]:
        x, y, w, h = e["rect"]
        if e["display"] != "none" and 20 <= w <= 60 and y > 700:
            real = e["rect"]; break
    if not real:
        print(json.dumps({"error": "no real button found", "rep": rep}, ensure_ascii=False)); return

    cx = int((real[0] + real[2] / 2) * dpr)
    cy = int((real[1] + real[3] / 2) * dpr)
    rep["click_at_phys"] = [cx, cy]
    move_cursor(cx, cy)
    time.sleep(0.6)
    rep["cursor"] = list(cursor_pos())
    phys_click()
    time.sleep(0.6)
    st2 = json.loads(cdp.evaluate(JS_STATE))
    rep["after"] = {"menu": st2["startMenu"], "rootClass": st2["rootClass"]}
    grab("_attic/qa/m4_a6_9_REAL_CLICK.png")
    print(json.dumps(rep, ensure_ascii=False, indent=1))

if __name__ == "__main__":
    main()
