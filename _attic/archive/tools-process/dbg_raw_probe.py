# -*- coding: utf-8 -*-
"""Raw CDP probe: is the page's JS loop responsive? Then try pause."""
import sys, time, io, json
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
import websocket

WS = "ws://127.0.0.1:9222/devtools/page/588B0E6149376C8AC9D1BFB7551609A5"
c = websocket.create_connection(WS, timeout=6, suppress_origin=True)
mid = 0

def call(method, params=None, wait=6.0):
    global mid
    mid += 1
    my = mid
    c.send(json.dumps({"id": my, "method": method, "params": params or {}}))
    t0 = time.time()
    while time.time() - t0 < wait:
        try:
            m = json.loads(c.recv())
        except websocket.WebSocketTimeoutException:
            return ("TIMEOUT", None)
        if m.get("id") == my:
            return ("OK", m)
        # stash events we care about
        if m.get("method") == "Debugger.paused":
            return ("PAUSED", m)
    return ("TIMEOUT", None)

print("Runtime.evaluate 1+1 ->", call("Runtime.evaluate", {"expression": "1+1", "returnByValue": True}))
print("Debugger.enable ->", call("Debugger.enable")[0])
send_pause = call("Debugger.pause", wait=5.0)
if send_pause[0] == "PAUSED":
    frames = send_pause[1]["params"].get("callFrames", [])[:10]
    for f in frames:
        loc = f.get("location", {})
        print("  ", f.get("functionName") or "(anon)", "@", loc.get("scriptId"), loc.get("lineNumber"))
    call("Debugger.resume")
else:
    print("Debugger.pause ->", send_pause[0])
