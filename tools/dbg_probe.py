# -*- coding: utf-8 -*-
import sys, time, io
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
from playwright.sync_api import sync_playwright
with sync_playwright() as p:
    b = p.chromium.connect_over_cdp("http://127.0.0.1:9222", timeout=15000)
    page = b.contexts[0].pages[0]
    print("connected, url:", page.url)
    t0 = time.time()
    print("eval:", page.evaluate("1+1"), "in", round(time.time()-t0, 2), "s")
