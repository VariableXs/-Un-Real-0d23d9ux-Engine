# -*- coding: utf-8 -*-
"""Verify UI stays responsive during heavy Rust commands (add-software / wallpaper scan)."""
import sys, time, io
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
from playwright.sync_api import sync_playwright

CHECK = """(async () => {
  const g = window.__TAURI__ || window.__TAURI_INTERNALS__;
  return { hasTauri: !!g, icons: document.querySelectorAll('.desktop-icon').length };
})()"""

with sync_playwright() as p:
    b = p.chromium.connect_over_cdp("http://127.0.0.1:9222", timeout=20000)
    page = b.contexts[0].pages[0]
    page.wait_for_selector('[data-testid="desktop-icons"]', timeout=90000)
    print("desktop ready")

    info = page.evaluate(CHECK)
    print("env:", info)

    if info.get("hasTauri"):
        # kick off heavy wallpaper scan in background (do NOT await it here)
        page.evaluate("""(async () => {
          window.__freezeProbe = { done: false, err: null, n: null };
          try {
            const inv = (window.__TAURI__ && window.__TAURI__.core) ? window.__TAURI__.core.invoke : window.__TAURI_INTERNALS__.invoke;
            const r = await inv('wp_engine_scan', { root: '' });
            window.__freezeProbe.n = r.length;
          } catch (e) { window.__freezeProbe.err = String(e); }
          window.__freezeProbe.done = true;
        })()""")
        worst = 0
        for i in range(12):
            t0 = time.time()
            page.evaluate("1+1")
            dt = time.time() - t0
            worst = max(worst, dt)
            print(f"probe {i+1}: {dt*1000:.0f}ms")
            time.sleep(0.5)
        st = page.evaluate("window.__freezeProbe")
        print("scan result:", st)
        print(f"WORST main-thread latency: {worst*1000:.0f}ms -> " +
              ("PASS (responsive)" if worst < 0.5 else "FAIL (still freezing)"))
    else:
        print("no global __TAURI__ — need UI-level test")
