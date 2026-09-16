# -*- coding: utf-8 -*-
# M4-A6 physical click chain probe (read-only until the single physical click).
# Stages: baseline -> summon(hotzone) -> hover V button -> read EXSTYLE ->
#         physical click -> verdict + screenshots.
# Usage: <venv>/Scripts/python.exe m4_a6_probe.py
import ctypes, ctypes.wintypes as wt, json, socket, base64, os, struct, time, urllib.request

try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:
    ctypes.windll.user32.SetProcessDPIAware()
u32 = ctypes.windll.user32

GWL_EXSTYLE = -20
WS_EX_TRANSPARENT = 0x20
WS_EX_LAYERED = 0x80000
WS_EX_NOACTIVATE = 0x08000000

def exstyle(h):
    if hasattr(u32, "GetWindowLongPtrW"):
        return u32.GetWindowLongPtrW(h, GWL_EXSTYLE) & 0xFFFFFFFF
    return u32.GetWindowLongW(h, GWL_EXSTYLE) & 0xFFFFFFFF

def find_taskbar_hwnds():
    out = []
    EnumProc = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def cb(h, _):
        n = u32.GetWindowTextLengthW(h)
        if n:
            b = ctypes.create_unicode_buffer(n + 1)
            u32.GetWindowTextW(h, b, n + 1)
            if "variable taskbar" in b.value.lower():
                out.append(h)
        return True
    u32.EnumWindows(EnumProc(cb), 0)
    return out

def rect_of(h):
    rc = wt.RECT()
    return [rc.left, rc.top, rc.right, rc.bottom] if u32.GetWindowRect(h, ctypes.byref(rc)) else None

def move_cursor(x, y):
    u32.SetCursorPos(int(x), int(y))

def phys_click():
    u32.mouse_event(0x02, 0, 0, 0, 0)  # LEFTDOWN
    time.sleep(0.05)
    u32.mouse_event(0x04, 0, 0, 0, 0)  # LEFTUP

def cursor_pos():
    pt = wt.POINT()
    u32.GetCursorPos(ctypes.byref(pt))
    return (pt.x, pt.y)

def grab(path):
    try:
        from PIL import ImageGrab
        ImageGrab.grab().save(path)
        return True
    except Exception as e:
        print("grab-fail:", e)
        return False

# --- CDP over raw WebSocket (stdlib only) ---
class CDP:
    def __init__(self, port=9223, url_sub="taskbar"):
        pages = json.loads(urllib.request.urlopen(f"http://127.0.0.1:{port}/json").read())
        tgt = [p for p in pages if url_sub in p.get("url", "")]
        if not tgt:
            raise RuntimeError(f"no '{url_sub}' page among {[p.get('url') for p in pages]}")
        wsurl = tgt[0]["webSocketDebuggerUrl"]
        hostport, path = wsurl.split("//", 1)[1].split("/", 1)
        host, port2 = hostport.split(":")
        self.mid = 0
        self.sock = socket.create_connection((host, int(port2)), timeout=8)
        key = base64.b64encode(os.urandom(16)).decode()
        req = (f"GET /{path} HTTP/1.1\r\nHost: {hostport}\r\n"
               "Upgrade: websocket\r\nConnection: Upgrade\r\n"
               f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n")
        self.sock.sendall(req.encode())
        resp = b""
        while b"\r\n\r\n" not in resp:
            chunk = self.sock.recv(4096)
            if not chunk:
                raise ConnectionError("handshake closed")
            resp += chunk
        if b"101" not in resp.split(b"\r\n")[0]:
            raise RuntimeError("ws handshake rejected: " + resp[:120].decode("replace"))

    def _rd(self, n):
        buf = b""
        while len(buf) < n:
            c = self.sock.recv(n - len(buf))
            if not c:
                raise ConnectionError("closed")
            buf += c
        return buf

    def send(self, obj):
        data = json.dumps(obj).encode()
        head = bytearray([0x81])
        mask = os.urandom(4)
        n = len(data)
        if n < 126:
            head.append(0x80 | n)
        elif n < 65536:
            head.append(0x80 | 126); head += struct.pack(">H", n)
        else:
            head.append(0x80 | 127); head += struct.pack(">Q", n)
        head += mask
        self.sock.sendall(bytes(head) + bytes(b ^ mask[i % 4] for i, b in enumerate(data)))

    def recv_msg(self):
        b1, b2 = self._rd(2)
        op = b1 & 0x0F
        ln = b2 & 0x7F
        if ln == 126:
            ln = struct.unpack(">H", self._rd(2))[0]
        elif ln == 127:
            ln = struct.unpack(">Q", self._rd(8))[0]
        payload = self._rd(ln)
        if op == 0x9:  # ping -> pong
            self.sock.sendall(bytes([0x8A, 0x80]) + os.urandom(4))
            return self.recv_msg()
        return json.loads(payload.decode("utf-8", "replace"))

    def evaluate(self, expr):
        self.mid += 1
        mid = self.mid
        self.send({"id": mid, "method": "Runtime.evaluate",
                   "params": {"expression": expr, "returnByValue": True}})
        deadline = time.time() + 8
        while time.time() < deadline:
            msg = self.recv_msg()
            if msg.get("id") == mid:
                res = msg.get("result", {})
                if "exceptionDetails" in res:
                    raise RuntimeError("JS exception: " + json.dumps(res["exceptionDetails"])[:400])
                return res.get("result", {}).get("value")
        raise TimeoutError("cdp evaluate timeout")

# --- JS probes ---
JS_STATE = r"""JSON.stringify((() => {
  const root = document.querySelector('[data-testid="taskbar-window-root"]');
  const btn = document.querySelector('[aria-label="\u5f00\u59cb"]');
  const r = btn ? btn.getBoundingClientRect() : null;
  return {
    rootClass: root ? root.className : null,
    btn: r ? [Math.round(r.x), Math.round(r.y), Math.round(r.width), Math.round(r.height)] : null,
    startMenu: !!document.querySelector('.start-menu'),
    quick: !!document.querySelector('.quick-panel'),
    innerW: window.innerWidth, innerH: window.innerHeight,
    dpr: window.devicePixelRatio
  };
})())"""

def main():
    rep = {"stages": []}
    hwnds = find_taskbar_hwnds()
    rep["tb_hwnds"] = [{"hwnd": h, "rect": rect_of(h),
                        "trans": bool(exstyle(h) & WS_EX_TRANSPARENT),
                        "layered": bool(exstyle(h) & WS_EX_LAYERED),
                        "noact": bool(exstyle(h) & WS_EX_NOACTIVATE)} for h in hwnds]
    if not hwnds:
        print(json.dumps(rep, ensure_ascii=False, indent=1)); return
    tb = hwnds[0]

    cdp = CDP()
    st0 = json.loads(cdp.evaluate(JS_STATE))
    rep["stages"].append({"name": "baseline", "state": st0,
                          "ex_trans": bool(exstyle(tb) & WS_EX_TRANSPARENT)})
    if not st0.get("btn"):
        print(json.dumps(rep, ensure_ascii=False, indent=1)); return

    # If a start menu is left open from earlier experiments, close it first
    # (toggle via CDP click) so the physical click must OPEN it.
    if st0.get("startMenu"):
        cdp.evaluate(
            "document.querySelector('[aria-label=\"\\u5f00\\u59cb\"]').click(); 'ok'")
        time.sleep(0.5)
        st0b = json.loads(cdp.evaluate(JS_STATE))
        rep["stages"].append({"name": "menu_closed_pre", "state": st0b})

    dpr = st0.get("dpr") or 1.25

    # Stage 1: summon via hotzone (screen bottom edge)
    sw = u32.GetSystemMetrics(0); sh = u32.GetSystemMetrics(1)
    move_cursor(sw // 2, sh - 3)
    time.sleep(0.9)
    st1 = json.loads(cdp.evaluate(JS_STATE))
    rep["stages"].append({"name": "summoned", "state": st1,
                          "ex_trans": bool(exstyle(tb) & WS_EX_TRANSPARENT)})
    grab("_attic/qa/m4_a6_1_summoned.png")

    if not st1.get("btn") or st1["btn"][1] > sh / dpr:
        rep["verdict"] = {"error": "taskbar not in shown state after summon",
                          "state": st1}
        print(json.dumps(rep, ensure_ascii=False, indent=1)); return

    bx, by, bw, bh = st1["btn"]  # shown-state geometry
    cx_phys = (bx + bw / 2) * dpr
    cy_phys = (by + bh / 2) * dpr

    # Stage 2: hover over the V (start) button, wait 2+ watcher cycles
    move_cursor(cx_phys, cy_phys)
    time.sleep(0.55)
    cp = cursor_pos()
    ex = exstyle(tb)
    st2 = json.loads(cdp.evaluate(JS_STATE))
    # in_hit prediction: cursor logic point inside button rect?
    fx, fy = cp[0] / dpr, cp[1] / dpr
    in_btn = (bx <= fx <= bx + bw) and (by <= fy <= by + bh)
    rep["stages"].append({"name": "hover_btn",
        "cursor_phys": list(cp), "cursor_logic": [round(fx, 1), round(fy, 1)],
        "btn_rect_logic": [bx, by, bw, bh], "in_btn_predicted": in_btn,
        "ex_trans": bool(ex & WS_EX_TRANSPARENT),
        "ex_layered": bool(ex & WS_EX_LAYERED),
        "ex_noact": bool(ex & WS_EX_NOACTIVATE),
        "state": st2})
    grab("_attic/qa/m4_a6_2_hover.png")

    # Stage 3: physical click at the button center
    phys_click()
    time.sleep(0.45)
    ex3 = exstyle(tb)
    st3 = json.loads(cdp.evaluate(JS_STATE))
    rep["stages"].append({"name": "after_click", "state": st3,
                          "ex_trans": bool(ex3 & WS_EX_TRANSPARENT)})
    grab("_attic/qa/m4_a6_3_afterclick.png")

    rep["verdict"] = {
        "menu_opened_by_physical_click": bool(st3.get("startMenu")),
        "hover_transparent_bit": rep["stages"][-2]["ex_trans"],
    }
    print(json.dumps(rep, ensure_ascii=False, indent=1))
    return

if __name__ == "__main__":
    main()
