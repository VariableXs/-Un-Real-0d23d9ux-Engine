# -*- coding: utf-8 -*-
"""Full real-input E2E: Escape -> real right-click -> real click 添加软件 -> real click launcher 添加软件."""
import sys, time, io, subprocess
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
from playwright.sync_api import sync_playwright

PS = ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass",
      "-File", r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\tools\realclick_lib.ps1"]

def real_click(x, y, right=False):
    cmd = PS + [str(int(x)), str(int(y))]
    subprocess.run(cmd, capture_output=True, text=True)

with sync_playwright() as p:
    browser = p.chromium.connect_over_cdp("http://127.0.0.1:9222")
    page = browser.contexts[0].pages[0]
    errs = []
    page.on("pageerror", lambda e: errs.append(str(e)))

    # close wallpaper center with Escape
    page.keyboard.press("Escape"); page.wait_for_timeout(500)
    print("wp-center closed")

    box = page.locator('[data-testid="desktop-icons"]').bounding_box()
    cx, cy = box["x"] + box["width"]*0.55, box["y"] + box["height"]*0.60
    real_click(cx, cy, right=True)
    page.wait_for_timeout(600)
    menu = page.locator(".ctx-menu").first
    print("menu open:", menu.is_visible())

    item = page.get_by_role("menuitem", name="添加软件").first
    bb = item.bounding_box()
    print("addApp bbox:", bb)
    t0 = time.time()
    real_click(bb["x"] + bb["width"]/2, bb["y"] + bb["height"]/2)
    print("clicked addApp (real)")
    page.wait_for_timeout(1200)
    print("responsive:", page.evaluate("1+1") == 2, "elapsed", round(time.time()-t0, 2))
    dlg_visible = page.get_by_role("dialog", name="软件管理").first.is_visible()
    print("launcher dialog visible:", dlg_visible)
    page.screenshot(path=r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\tools\real6_launcher.png")

    # click launcher's inner 添加软件 button (real)
    btn = page.get_by_role("button", name="添加软件").first
    b2 = btn.bounding_box()
    print("launcher button bbox:", b2)
    t0 = time.time()
    real_click(b2["x"] + b2["width"]/2, b2["y"] + b2["height"]/2)
    print("clicked launcher addApp (real)")
    # poll responsiveness for 10s
    hung = False
    for i in range(20):
        time.sleep(0.5)
        try:
            ok = page.evaluate("1+1") == 2
        except Exception as e:
            ok = False
        if not ok:
            hung = True
            print(f"t+{0.5*(i+1):.1f}s NOT RESPONDING")
        elif i % 4 == 3:
            print(f"t+{0.5*(i+1):.1f}s ok")
    print("FROZE:", hung, "elapsed", round(time.time()-t0, 2))
    page.screenshot(path=r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\tools\real7_dialog.png")
    for e in errs[-10:]:
        print("pageerror:", e)
