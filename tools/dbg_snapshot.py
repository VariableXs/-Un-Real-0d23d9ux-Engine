# -*- coding: utf-8 -*-
import sys, io
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
from playwright.sync_api import sync_playwright

with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp("http://127.0.0.1:9222")
    page = browser.contexts[0].pages[0]
    print("URL:", page.url)
    print("BODY:", page.evaluate("document.body.className"))
    print("HTML head:", page.evaluate("document.body.innerHTML.slice(0, 600)"))
    page.screenshot(path="d:/2/14/-Un-Real-0d23d9ux-Engine-main/tools/dbg0_state.png")
