# -*- coding: utf-8 -*-
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
    page.wait_for_selector('[data-testid="desktop-icons"]', timeout=60000)
    cdp = b.contexts[0].new_cdp_session(page)
    got = {}
    def on_script(e):
        got[e["scriptId"]] = e.get("url", "")
    cdp.on("Debugger.scriptParsed", on_script)
    cdp.send("Debugger.enable")
    time.sleep(2)  # let scriptParsed events flush
    print("script 430 url:", got.get("430", "NOT TRACKED"))

    paused = {}
    def on_paused(params):
        frames = []
        for f in params.get("callFrames", [])[:30]:
            loc = f.get("location", {})
            frames.append((f.get('functionName') or '(anon)', loc.get('scriptId'), loc.get('lineNumber'), loc.get('columnNumber')))
        paused["frames"] = frames
        cdp.send("Debugger.resume")
    cdp.on("Debugger.paused", on_paused)

    box = page.locator('[data-testid="desktop-icons"]').bounding_box()
    cx, cy = box["x"] + box["width"]*0.55, box["y"] + box["height"]*0.60
    real_click(cx, cy, right=True)
    time.sleep(3)
    try:
        page.evaluate("1+1", timeout=3000)
        print("responsive?!")
    except Exception:
        print("FROZEN")
        for attempt in range(3):
            cdp.send("Debugger.pause")
            time.sleep(1.5)
            if "frames" in paused:
                for fn, sid, ln, col in paused["frames"]:
                    url = got.get(str(sid), "?")
                    print(f"  {fn} @ {url}:{ln}:{col}")
                # dump source context of deepest frame
                sid0 = str(paused["frames"][0][1]); ln0 = paused["frames"][0][2]
                src = cdp.send("Debugger.getScriptSource", {"scriptId": sid0})["scriptSource"]
                lines = src.split("\n")
                for i in range(max(0, ln0-8), min(len(lines), ln0+5)):
                    print(f"  {'>>' if i == ln0 else '  '} {i+1}: {lines[i][:160]}")
                break
        else:
            print("no pause response")
