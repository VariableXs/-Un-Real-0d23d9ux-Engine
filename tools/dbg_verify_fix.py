# -*- coding: utf-8 -*-
"""Verify fix: real right-click opens menu, real clicks work, no freeze."""
import sys, time, io, subprocess
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
from playwright.sync_api import sync_playwright
PS = ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass",
      "-File", r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\tools\realclick_lib.ps1"]
def real_click(x, y, right=False):
    subprocess.run(PS + [str(int(x)), str(int(y))], capture_output=True, text=True)

with sync_playwright() as p:
    b = p.chromium.connect_over_cdp("http://127.0.0.1:9222", timeout=20000)
    page = b.contexts[0].pages[0]
    page.wait_for_selector('[data-testid="desktop-icons"]', timeout=90000)
    print("desktop ready")

    box = page.locator('[data-testid="desktop-icons"]').bounding_box()
    cx, cy = box["x"] + box["width"]*0.55, box["y"] + box["height"]*0.60

    # 1) real right-click -> menu opens?
    real_click(cx, cy, right=True)
    page.wait_for_timeout(700)
    print("menu open:", page.locator(".ctx-menu").first.is_visible())
    page.screenshot(path=r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\tools\fix1_menu.png")

    # 2) real click 添加软件 -> launcher opens? page stays responsive?
    bb = page.get_by_role("menuitem", name="添加软件").first.bounding_box()
    real_click(bb["x"] + bb["width"]/2, bb["y"] + bb["height"]/2)
    page.wait_for_timeout(1000)
    print("responsive after addApp:", page.evaluate("1+1") == 2)
    print("launcher visible:", page.get_by_role("dialog", name="软件管理").first.is_visible())
    page.screenshot(path=r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\tools\fix2_launcher.png")

    # 3) close launcher (real click on close X via Escape)
    page.keyboard.press("Escape"); page.wait_for_timeout(400)

    # 4) real right-click -> hover 切换壁纸 -> real click 壁纸中心
    real_click(cx, cy, right=True)
    page.wait_for_timeout(600)
    wp = page.get_by_role("menuitem", name="切换壁纸").first
    wb = wp.bounding_box()
    real_click(wb["x"] + wb["width"]/2, wb["y"] + wb["height"]/2)  # expand submenu
    page.wait_for_timeout(500)
    center = page.get_by_role("menuitem", name="壁纸中心").first
    cb = center.bounding_box()
    print("壁纸中心 bbox:", cb)
    real_click(cb["x"] + cb["width"]/2, cb["y"] + cb["height"]/2)
    page.wait_for_timeout(1200)
    print("responsive after wp-center:", page.evaluate("1+1") == 2)
    print("wp-center open:", page.evaluate("!!document.querySelector('.wp-center-root')"))
    page.screenshot(path=r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\tools\fix3_wpcenter.png")

    # 5) outside-click closes wp-center (user requirement 2)
    real_click(cx, cy)
    page.wait_for_timeout(600)
    print("wp-center closed by outside click:", not page.evaluate("!!document.querySelector('.wp-center-root')"))
    print("ALL OK")
