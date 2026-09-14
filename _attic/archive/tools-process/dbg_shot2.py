# -*- coding: utf-8 -*-
import sys, io
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
from playwright.sync_api import sync_playwright
with sync_playwright() as p:
    b = p.chromium.connect_over_cdp("http://127.0.0.1:9222", timeout=15000)
    page = b.contexts[0].pages[0]
    print("responsive:", page.evaluate("1+1") == 2)
    print("menus:", page.locator(".ctx-menu").count())
    print("body classes:", page.evaluate("document.body.className"))
    page.screenshot(path=r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\tools\fix1_state.png")
