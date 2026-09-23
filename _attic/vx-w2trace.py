import os, sys, datetime, traceback

LOG = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-w2trace.rpt"
def log(msg=""):
    with open(LOG, "a", encoding="utf-8") as f:
        f.write(str(msg) + "\n")

try:
    log("== webview trace " + datetime.datetime.now().isoformat())
    base = r"X:\Users\Varia\AppData\Local"
    if not os.path.isdir(base):
        log("base missing")
    else:
        for e in sorted(os.listdir(base)):
            p = os.path.join(base, e)
            try:
                mt = datetime.datetime.fromtimestamp(os.path.getmtime(p))
            except Exception:
                mt = "?"
            log(f"{e}  {mt}")
        log("-- candidates --")
        for e in os.listdir(base):
            if any(k in e.lower() for k in ("variable", "varix", "varia", "webview", "tauri")):
                p = os.path.join(base, e)
                log("CAND: " + p + " " + datetime.datetime.fromtimestamp(os.path.getmtime(p)).isoformat())
                if os.path.isdir(p):
                    for root2, dirs2, files2 in os.walk(p):
                        depth = root2[len(p):].count(os.sep)
                        if depth > 2:
                            dirs2.clear()
                        for f in files2[:8]:
                            fp = os.path.join(root2, f)
                            try:
                                mt = datetime.datetime.fromtimestamp(os.path.getmtime(fp))
                            except Exception:
                                mt = "?"
                            log("   " + fp[len(p):] + "  " + str(mt))
    # ProgramData traces
    for pd in (r"X:\ProgramData", r"X:\Windows\Temp"):
        if not os.path.isdir(pd):
            continue
        hits = [e for e in os.listdir(pd) if any(k in e.lower() for k in ("variable", "varix", "tauri"))]
        if hits:
            log("PD hits " + pd + ": " + repr(hits))
    log("done")
except Exception:
    log("EXC " + traceback.format_exc()[:1200])
    log("done")
