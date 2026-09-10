# -*- coding: utf-8 -*-
import sys, time, io
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
from playwright.sync_api import sync_playwright

LOG = []
with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp("http://127.0.0.1:9222")
    ctx = browser.contexts[0]
    page = ctx.pages[0]
    page.on("console", lambda m: LOG.append(f"[console.{m.type}] {m.text}"))
    page.on("pageerror", lambda e: LOG.append(f"[pageerror] {e}"))

    # close any open menu / launcher
    page.keyboard.press("Escape"); page.wait_for_timeout(300)

    # open launcher from taskbar? simpler: right-click desktop -> 添加软件
    box = page.locator('[data-testid="desktop-icons"]').bounding_box()
    cx, cy = box["x"] + box["width"]*0.55, box["y"] + box["height"]*0.6
    page.mouse.click(cx, cy, button="right"); page.wait_for_timeout(300)
    page.get_by_role("menuitem", name="添加软件").first.click(); page.wait_for_timeout(800)

    btn = page.get_by_role("button", name="添加软件")
    print("launcher addApp buttons:", btn.count())
    t0 = time.time()
    btn.first.click(timeout=5000)
    print("clicked launcher addApp")
    # poll responsiveness + look for native dialog window
    for i in range(12):
        time.sleep(0.5)
        try:
            ok = page.evaluate("1+1") == 2
        except Exception as e:
            ok = f"EVAL FAIL: {e}"
        print(f"t+{0.5*(i+1):.1f}s responsive={ok}")
    page.screenshot(path="d:/2/14/-Un-Real-0d23d9ux-Engine-main/tools/dbg4_dialog.png")
    # enumerate windows to see if a file dialog exists
    for l in LOG[-30:]:
        print(l)
