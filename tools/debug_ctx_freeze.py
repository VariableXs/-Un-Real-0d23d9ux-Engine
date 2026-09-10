# -*- coding: utf-8 -*-
"""Reproduce desktop context-menu freeze: connect over CDP to running Variable."""
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
    print("URL:", page.url)
    # wait for desktop ready
    page.wait_for_selector('[data-testid="desktop-icons"]', timeout=60000)
    print("desktop ready")

    # 1) right-click empty desktop area (bottom-center of canvas)
    box = page.locator('[data-testid="desktop-icons"]').bounding_box()
    print("desktop box:", box)
    cx = box["x"] + box["width"] * 0.55
    cy = box["y"] + box["height"] * 0.6
    page.mouse.click(cx, cy, button="right")
    page.wait_for_timeout(400)
    menu = page.locator(".ctx-menu").first
    print("menu visible:", menu.is_visible())
    page.screenshot(path="d:/2/14/-Un-Real-0d23d9ux-Engine-main/tools/dbg1_menu.png")

    # 2) click 添加软件
    t0 = time.time()
    item = page.get_by_role("menuitem", name="添加软件")
    print("addApp count:", item.count())
    item.first.click(timeout=5000)
    print("clicked in", round(time.time() - t0, 2), "s")
    page.wait_for_timeout(1500)
    alive = page.evaluate("1+1")
    print("page responsive after addApp:", alive == 2, "elapsed", round(time.time() - t0, 2))
    page.screenshot(path="d:/2/14/-Un-Real-0d23d9ux-Engine-main/tools/dbg2_addapp.png")

    # 3) close launcher, retry wallpaper submenu
    page.keyboard.press("Escape")
    page.wait_for_timeout(300)
    page.mouse.click(cx, cy, button="right")
    page.wait_for_timeout(400)
    wp = page.get_by_role("menuitem", name="切换壁纸")
    print("wp count:", wp.count())
    t0 = time.time()
    wp.first.click(timeout=5000)
    page.wait_for_timeout(600)
    print("after wp click responsive:", page.evaluate("1+1") == 2, round(time.time() - t0, 2))
    page.screenshot(path="d:/2/14/-Un-Real-0d23d9ux-Engine-main/tools/dbg3_wp.png")

    for l in LOG[-40:]:
        print(l)
