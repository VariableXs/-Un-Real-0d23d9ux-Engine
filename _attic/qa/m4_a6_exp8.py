# -*- coding: utf-8 -*-
# M4-A6 experiment 8: measure ALL taskbar buttons - who really sits at the click point?
import json, time, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from m4_a6_probe import CDP, JS_STATE, move_cursor, u32

JS_BTNS = r"""JSON.stringify((() => {
  const out = [];
  document.querySelectorAll('button, [role="button"]').forEach(b => {
    const r = b.getBoundingClientRect();
    if (r.width < 1 && r.height < 1) return;
    const cs = getComputedStyle(b);
    out.push({
      label: b.getAttribute('aria-label') || b.title || b.textContent.trim().slice(0, 12),
      cls: (b.className || '').toString().slice(0, 40),
      rect: [Math.round(r.x), Math.round(r.y), Math.round(r.width), Math.round(r.height)],
      pe: cs.pointerEvents, vis: cs.display !== 'none' && cs.visibility !== 'hidden'
    });
  });
  // hit-test the three candidate points
  const hit = x => { const e = document.elementFromPoint(x, 833);
    return e ? (e.className || e.tagName).toString().slice(0, 40) : null; };
  return {
    btns: out,
    hit_at_768: hit(768), hit_at_557: hit(557), hit_at_620: hit(620),
    hit_at_680: hit(680), hit_at_740: hit(740)
  };
})())"""

def main():
    cdp = CDP()
    sw = u32.GetSystemMetrics(0); sh = u32.GetSystemMetrics(1)
    move_cursor(sw // 2, sh - 3)
    time.sleep(0.9)
    st1 = json.loads(cdp.evaluate(JS_STATE))
    print(json.dumps(json.loads(cdp.evaluate(JS_BTNS)), ensure_ascii=False, indent=1))

if __name__ == "__main__":
    main()
