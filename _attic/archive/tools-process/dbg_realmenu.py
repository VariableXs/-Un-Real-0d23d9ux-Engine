# -*- coding: utf-8 -*-
import sys, time, io, subprocess
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
from playwright.sync_api import sync_playwright
PS = ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass",
      "-File", r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\tools\realclick_lib.ps1"]
def real_click(x, y, right=False):
    subprocess.run(PS + [str(int(x)), str(int(y))], capture_output=True, text=True)

with sync_playwright() as p:
    b = p.chromium.connect_over_cdp("http://127.0.0.1:9222")
    page = b.contexts[0].pages[0]
    page.evaluate("""
      window.__evlog = [];
      ['pointerdown','pointerup','mousedown','mouseup','contextmenu','click'].forEach(t =>
        window.addEventListener(t, (e) => {
          if (window.__evlog.length > 60) window.__evlog.shift();
          window.__evlog.push(`${t} btn=${e.button} xy=${e.clientX},${e.clientY} tgt=${e.target.className && e.target.className.baseVal !== undefined ? 'svg' : (e.target.className || e.target.tagName)}`);
        }, true));
    """)
    box = page.locator('[data-testid="desktop-icons"]').bounding_box()
    cx, cy = box["x"] + box["width"]*0.55, box["y"] + box["height"]*0.60
    real_click(cx, cy, right=True)
    page.wait_for_timeout(700)
    print("menu count:", page.locator(".ctx-menu").count())
    for l in page.evaluate("window.__evlog"):
        print(l)
    page.screenshot(path=r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\tools\real9.png")
