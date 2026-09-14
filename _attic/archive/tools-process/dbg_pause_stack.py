# -*- coding: utf-8 -*-
"""Reproduce freeze, then Debugger.pause to capture the stuck JS stack."""
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
    logs = []
    page.on("console", lambda m: logs.append(f"[{m.type}] {m.text}"))
    page.wait_for_selector('[data-testid="desktop-icons"]', timeout=60000)
    print("desktop ready")

    cdp = b.contexts[0].new_cdp_session(page)
    paused = {}
    def on_paused(params):
        frames = []
        for f in params.get("callFrames", [])[:30]:
            loc = f.get("location", {})
            frames.append(f"{f.get('functionName') or '(anon)'} @ {loc.get('scriptId','?')}:{loc.get('lineNumber')}:{loc.get('columnNumber')}")
        paused["frames"] = frames
        cdp.send("Debugger.resume")
    cdp.on("Debugger.paused", on_paused)
    cdp.send("Debugger.enable")

    box = page.locator('[data-testid="desktop-icons"]').bounding_box()
    cx, cy = box["x"] + box["width"]*0.55, box["y"] + box["height"]*0.60
    real_click(cx, cy, right=True)
    time.sleep(3)
    # probe liveness
    try:
        page.evaluate("1+1", timeout=3000)
        print("still responsive after right-click?!")
    except Exception:
        print("FROZEN — sending Debugger.pause")
        for attempt in range(3):
            cdp.send("Debugger.pause")
            time.sleep(1.5)
            if "frames" in paused:
                print("--- STUCK STACK ---")
                for f in paused["frames"]:
                    print("  ", f)
                break
        else:
            print("pause got no response")

    for l in logs[-25:]:
        print(l)
