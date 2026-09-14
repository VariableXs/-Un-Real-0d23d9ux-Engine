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
    x = page.locator(".wp-center-close, button:has-text('✕')").first
    bb = x.bounding_box()
    print("close btn:", bb)
    if bb:
        real_click(bb["x"] + bb["width"]/2, bb["y"] + bb["height"]/2)
        page.wait_for_timeout(600)
    print("wp-center still open:", page.evaluate("!!document.querySelector('.wp-center-root') && document.querySelector('.wp-center-root').className"))
    # now real right-click desktop
    box = page.locator('[data-testid="desktop-icons"]').bounding_box()
    cx, cy = box["x"] + box["width"]*0.55, box["y"] + box["height"]*0.60
    real_click(cx, cy, right=True)
    page.wait_for_timeout(700)
    print("menu open:", page.locator(".ctx-menu").first.is_visible())
    page.screenshot(path=r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\tools\real8_menu.png")
