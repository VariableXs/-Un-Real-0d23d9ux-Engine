"""GUI probe helper for Variable QA. ASCII-only.
Usage:
  py gui_probe.py shot <out.png> [x y w h]
  py gui_probe.py click <x> <y>
  py gui_probe.py dclick <x> <y>
  py gui_probe.py rclick <x> <y>
  py gui_probe.py move <x> <y>
  py gui_probe.py drag <x1> <y1> <x2> <y2>
  py gui_probe.py type <text>
  py gui_probe.py key <keyname>
  py gui_probe.py combo <k1,k2,...>
  py gui_probe.py scroll <amount> [x y]
  py gui_probe.py wins            # list top-level windows
  py gui_probe.py mouse           # print cursor pos
"""
import sys, os
import ctypes
from ctypes import wintypes
import pyautogui

os.environ.setdefault("PYAUTOGUI_PAUSE", "0.15")
pyautogui.FAILSAFE = False
pyautogui.PAUSE = 0.15

def wins():
    user32 = ctypes.windll.user32
    res = []
    @ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    def cb(h, l):
        if user32.IsWindowVisible(h):
            n = user32.GetWindowTextLengthW(h)
            if n > 0:
                buf = ctypes.create_unicode_buffer(n + 1)
                user32.GetWindowTextW(h, buf, n + 1)
                r = wintypes.RECT()
                user32.GetWindowRect(h, ctypes.byref(r))
                res.append((h, buf.value, r.left, r.top, r.right - r.left, r.bottom - r.top))
        return True
    user32.EnumWindows(cb, 0)
    for h, t, x, y, w, hh in res:
        print(f"{h}\t{x},{y}\t{w}x{hh}\t{t}")

def main():
    cmd = sys.argv[1]
    a = sys.argv[2:]
    if cmd == "shot":
        region = tuple(int(v) for v in a[1:5]) if len(a) >= 5 else None
        img = pyautogui.screenshot(region=region)
        d = os.path.dirname(a[0])
        if d: os.makedirs(d, exist_ok=True)
        img.save(a[0]); print("saved", a[0], img.size)
    elif cmd == "click":
        pyautogui.click(int(a[0]), int(a[1])); print("ok")
    elif cmd == "dclick":
        pyautogui.doubleClick(int(a[0]), int(a[1])); print("ok")
    elif cmd == "rclick":
        pyautogui.rightClick(int(a[0]), int(a[1])); print("ok")
    elif cmd == "move":
        pyautogui.moveTo(int(a[0]), int(a[1])); print("ok")
    elif cmd == "drag":
        pyautogui.moveTo(int(a[0]), int(a[1]))
        pyautogui.drag(int(a[2]) - int(a[0]), int(a[3]) - int(a[1]), duration=0.4); print("ok")
    elif cmd == "type":
        pyautogui.write(a[0], interval=0.02); print("ok")
    elif cmd == "key":
        pyautogui.press(a[0]); print("ok")
    elif cmd == "combo":
        pyautogui.hotkey(*a[0].split(",")); print("ok")
    elif cmd == "scroll":
        pos = (int(a[1]), int(a[2])) if len(a) >= 3 else None
        pyautogui.scroll(int(a[0]), *pos if pos else ()); print("ok")
    elif cmd == "wins":
        wins()
    elif cmd == "mouse":
        print(pyautogui.position())
    else:
        print("unknown cmd")

if __name__ == "__main__":
    main()
