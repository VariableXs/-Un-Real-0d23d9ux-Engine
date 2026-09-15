# M4 self-check helper: pure-ctypes screenshot + input, single physical coord space
import ctypes, ctypes.wintypes as wt, time, json, sys
try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:
    ctypes.windll.user32.SetProcessDPIAware()
u32 = ctypes.windll.user32
g32 = ctypes.windll.gdi32

SHOT = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"

def shot_ct(name):
    from PIL import Image
    w, h = u32.GetSystemMetrics(0), u32.GetSystemMetrics(1)
    hdc = u32.GetDC(0)
    mdc = g32.CreateCompatibleDC(hdc)
    bmp = g32.CreateCompatibleBitmap(hdc, w, h)
    g32.SelectObject(mdc, bmp)
    g32.BitBlt(mdc, 0, 0, w, h, hdc, 0, 0, 0x00CC0020)
    class BMPINFOHEADER(ctypes.Structure):
        _fields_ = [("biSize", ctypes.c_uint32), ("biWidth", ctypes.c_int32), ("biHeight", ctypes.c_int32),
                    ("biPlanes", ctypes.c_uint16), ("biBitCount", ctypes.c_uint16), ("biCompression", ctypes.c_uint32),
                    ("biSizeImage", ctypes.c_uint32), ("biXPelsPerMeter", ctypes.c_int32), ("biYPelsPerMeter", ctypes.c_int32),
                    ("biClrUsed", ctypes.c_uint32), ("biClrImportant", ctypes.c_uint32)]
    bi = BMPINFOHEADER(ctypes.sizeof(BMPINFOHEADER), w, -h, 1, 32, 0, 0, 0, 0, 0, 0)
    buf = ctypes.create_string_buffer(w * h * 4)
    g32.GetDIBits(mdc, bmp, 0, h, buf, ctypes.byref(bi), 0)
    img = Image.frombuffer("RGBA", (w, h), buf.raw, "raw", "BGRA", 0, 1)
    img.convert("RGB").save(f"{SHOT}/{name}.png")
    g32.DeleteObject(bmp); g32.DeleteDC(mdc); u32.ReleaseDC(0, hdc)
    return f"{SHOT}/{name}.png"

MOUSEEVENTF = {"move": 0x0001, "abs": 0x8000, "leftdown": 0x0002, "leftup": 0x0004, "rightdown": 0x0008, "rightup": 0x0010, "wheel": 0x0800}

def _mouse(dwflags, x=None, y=None, data=0):
    if x is not None:
        u32.SetCursorPos(int(x), int(y))
        time.sleep(0.06)
    u32.mouse_event(dwflags, 0, 0, data, 0)
    time.sleep(0.06)

def click_ct(x, y, times=1):
    for _ in range(times):
        _mouse(0x0002 | 0x0004, x, y)

def rclick_ct(x, y):
    _mouse(0x0008 | 0x0010, x, y)

def drag_ct(x0, y0, x1, y1, steps=24, hold=0.05):
    _mouse(0x0002, x0, y0)
    time.sleep(hold)
    for i in range(1, steps + 1):
        nx = x0 + (x1 - x0) * i / steps
        ny = y0 + (y1 - y0) * i / steps
        u32.SetCursorPos(int(nx), int(ny))
        time.sleep(0.02)
    time.sleep(hold)
    _mouse(0x0004)

def scroll_ct(x, y, amount):
    _mouse(0x0800, x, y, data=int(amount))

def rect_of(hwnd):
    rc = wt.RECT()
    u32.GetWindowRect(hwnd, ctypes.byref(rc))
    return [rc.left, rc.top, rc.right, rc.bottom]

def typewrite_ct(text, per_key=0.03):
    import keyboard_send as _  # noqa: F401 (placeholder, unused)

def send_text(text):
    # unicode SendInput per char
    class KI(ctypes.Structure):
        _fields_ = [("wVk", wt.WORD), ("wScan", wt.WORD), ("dwFlags", wt.DWORD), ("time", wt.DWORD), ("dwExtraInfo", ctypes.POINTER(ctypes.c_ulong))]
    class INPUT(ctypes.Structure):
        class U(ctypes.Union):
            _fields_ = [("ki", KI)]
        _anonymous_ = ("u",)
        _fields_ = [("type", wt.DWORD), ("u", U)]
    KEYEVENTF_UNICODE, KEYEVENTF_KEYUP = 0x0004, 0x0008
    arr = (INPUT * (len(text) * 2))()
    for i, ch in enumerate(text):
        arr[2 * i].type = 1
        arr[2 * i].ki = KI(0, ord(ch), KEYEVENTF_UNICODE, 0, None)
        arr[2 * i + 1].type = 1
        arr[2 * i + 1].ki = KI(0, ord(ch), KEYEVENTF_UNICODE | KEYEVENTF_KEYUP, 0, None)
    u32.SendInput(len(arr), ctypes.pointer(arr), ctypes.sizeof(INPUT))
    time.sleep(0.05 * len(text))

if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "shot":
        shot_ct(sys.argv[2])
        print("saved", sys.argv[2])
    elif cmd == "click":
        click_ct(int(sys.argv[2]), int(sys.argv[3]))
        print("clicked", sys.argv[2], sys.argv[3])
    elif cmd == "rect":
        print(json.dumps(rect_of(int(sys.argv[2]))))
