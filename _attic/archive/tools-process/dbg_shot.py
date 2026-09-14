# -*- coding: utf-8 -*-
import sys, io
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
from playwright.sync_api import sync_playwright
with sync_playwright() as p:
    b = p.chromium.connect_over_cdp("http://127.0.0.1:9222")
    page = b.contexts[0].pages[0]
    print("menu count:", page.locator(".ctx-menu").count())
    print("focus:", page.evaluate("document.activeElement ? document.activeElement.className : 'none'"))
    page.screenshot(path=r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\tools\state_now.png")
