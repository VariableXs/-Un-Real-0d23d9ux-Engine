# -*- coding: utf-8 -*-
"""Sample the frozen JS thread repeatedly to find the true spinner."""
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
    cdp = b.contexts[0].new_cdp_session(page)
    urls = {}
    def on_script(e):
        urls[e["scriptId"]] = e.get("url", "")
    cdp.on("Debugger.scriptParsed", on_script)
    cdp.send("Debugger.enable")
    time.sleep(2)

    box = page.locator('[data-testid="desktop-icons"]').bounding_box()
    cx, cy = box["x"] + box["width"]*0.55, box["y"] + box["height"]*0.60
    real_click(cx, cy, right=True)
    time.sleep(2.5)
    try:
        page.evaluate("1+1", timeout=3000)
        print("NOT frozen this time")
        sys.exit(0)
    except Exception:
        print("FROZEN — sampling 12 pauses")
    seen = []
    for i in range(12):
        got = {}
        def on_paused(params, got=got):
            for f in params.get("callFrames", [])[:12]:
                loc = f.get("location", {})
                fn = f.get("functionName") or "(anon)"
                got[f"{fn} @ {urls.get(str(loc.get('scriptId')),'?')}:{loc.get('lineNumber')}"] = True
            cdp.send("Debugger.resume")
        h = lambda params, got=got: on_paused(params, got)
        cdp.on("Debugger.paused", h)
        cdp.send("Debugger.pause")
        time.sleep(1.0)
        try:
            cdp.remove_listener("Debugger.paused", h)
        except Exception:
            pass
        for k in got:
            seen.append(k)
        time.sleep(0.3)
    from collections import Counter
    for k, n in Counter(seen).most_common(30):
        print(f"{n:3d}  {k}")

