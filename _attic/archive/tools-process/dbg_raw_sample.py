# -*- coding: utf-8 -*-
"""Raw CDP sampler: attach directly to the page ws endpoint, pause, dump stacks."""
import sys, time, io, json
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
import websocket

WS = "ws://127.0.0.1:9222/devtools/page/588B0E6149376C8AC9D1BFB7551609A5"
c = websocket.create_connection(WS, timeout=10, suppress_origin=True)
mid = 0
def send(method, params=None):
    global mid
    mid += 1
    c.send(json.dumps({"id": mid, "method": method, "params": params or {}}))
    return mid

send("Debugger.enable")
urls = {}
seen = []
# drain for scriptParsed
t0 = time.time()
while time.time() - t0 < 3:
    try:
        m = json.loads(c.recv())
        if m.get("method") == "Debugger.scriptParsed":
            urls[m["params"]["scriptId"]] = m["params"].get("url", "")
    except Exception:
        break

for i in range(12):
    send("Debugger.pause")
    got = {}
    t0 = time.time()
    while time.time() - t0 < 2.5:
        try:
            m = json.loads(c.recv())
        except Exception:
            break
        if m.get("method") == "Debugger.paused":
            for f in m["params"].get("callFrames", [])[:12]:
                loc = f.get("location", {})
                fn = f.get("functionName") or "(anon)"
                got[f"{fn} @ {urls.get(str(loc.get('scriptId')),'?')}:{loc.get('lineNumber')}"] = True
            send("Debugger.resume")
            break
    for k in got:
        seen.append(k)
    time.sleep(0.3)

from collections import Counter
for k, n in Counter(seen).most_common(30):
    print(f"{n:3d}  {k}")
