# -*- coding: utf-8 -*-
import sys, time, io
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
from playwright.sync_api import sync_playwright

LOG = []
with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp("http://127.0.0.1:9222")
    page = browser.contexts[0].pages[0]
    page.on("console", lambda m: LOG.append(f"[console.{m.type}] {m.text}"))
    page.on("pageerror", lambda e: LOG.append(f"[pageerror] {e}"))

    page.keyboard.press("Escape"); page.wait_for_timeout(300)
    box = page.locator('[data-testid="desktop-icons"]').bounding_box()
    cx, cy = box["x"] + box["width"]*0.55, box["y"] + box["height"]*0.6
    page.mouse.click(cx, cy, button="right"); page.wait_for_timeout(300)
    # hover 切换壁纸 to open submenu
    page.get_by_role("menuitem", name="切换壁纸").first.hover(); page.wait_for_timeout(400)
    wpcenter = page.get_by_role("menuitem", name="壁纸中心")
    print("壁纸中心 count:", wpcenter.count())
    t0 = time.time()
    wpcenter.first.click(timeout=5000)
    print("clicked 壁纸中心")
    for i in range(20):
        time.sleep(0.5)
        try:
            ok = page.evaluate("1+1") == 2
        except Exception as e:
            ok = f"EVAL FAIL: {e}"
        vis = False
        try:
            vis = page.locator(".wp-center, [class*='wp-center']").first.is_visible()
        except Exception:
            pass
        print(f"t+{0.5*(i+1):.1f}s responsive={ok} centerVisible={vis}")
        if vis and i > 2:
            break
    page.screenshot(path="d:/2/14/-Un-Real-0d23d9ux-Engine-main/tools/dbg5_wpcenter.png")
    for l in LOG[-30:]:
        print(l)
