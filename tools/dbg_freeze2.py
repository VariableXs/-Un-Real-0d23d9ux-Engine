# -*- coding: utf-8 -*-
"""Probe: is the app responsive right now? Sample stacks if frozen."""
import sys, time, io
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
from playwright.sync_api import sync_playwright

with sync_playwright() as p:
    b = p.chromium.connect_over_cdp("http://127.0.0.1:9222", timeout=20000)
    page = b.contexts[0].pages[0]
    try:
        t0 = time.time()
        r = page.evaluate("1+1", timeout=5000)
        print(f"responsive: {r} in {time.time()-t0:.2f}s")
    except Exception:
        print("FROZEN — sampling 12 pauses")
        cdp = b.contexts[0].new_cdp_session(page)
        urls = {}
        def on_script(e):
            urls[e["scriptId"]] = e.get("url", "")
        cdp.on("Debugger.scriptParsed", on_script)
        cdp.send("Debugger.enable")
        time.sleep(2)
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
