# R4 QA helper — 物理像素坐标（DPI aware），截图 + win32 枚举
import ctypes, sys
try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:
    ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time, json
pyautogui.FAILSAFE = False
pyautogui.PAUSE = 0.15
SHOT = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa/r4"

def shot(name):
    p = pyautogui.screenshot()
    p.save(f"{SHOT}/{name}.png")
    print("saved", name, p.size)

def click(x, y, name="", wait=0.9):
    pyautogui.click(x, y)
    print(f"click {name} ({x},{y})")
    time.sleep(wait)

def dclick(x, y, name="", wait=1.2):
    pyautogui.doubleClick(x, y)
    print(f"dclick {name} ({x},{y})")
    time.sleep(wait)

def rclick(x, y, name="", wait=0.9):
    pyautogui.rightClick(x, y)
    print(f"rclick {name} ({x},{y})")
    time.sleep(wait)

def key(*k, wait=0.8):
    pyautogui.hotkey(*k)
    print("key", "+".join(k))
    time.sleep(wait)

def press(k, wait=0.5, repeats=1):
    for _ in range(repeats):
        pyautogui.press(k)
    print("press", k, "x", repeats)
    time.sleep(wait)

def typewrite(s, wait=0.5):
    # 先确保英文态（ASCII 注入前置：按一次 shift 切换由调用方决定）
    pyautogui.typewrite(s, interval=0.04)
    print("typed", s)
    time.sleep(wait)

def enum_var_windows():
    import win32gui, win32process
    rows = []
    def cb(h, _):
        t = win32gui.GetWindowText(h)
        c = win32gui.GetClassName(h)
        if t or c == "Tauri Window":
            rows.append((h, c, t[:60], win32gui.IsWindowVisible(h)))
    win32gui.EnumWindows(cb, None)
    return rows

def var_rect():
    import win32gui
    out = []
    def cb(h, _):
        if win32gui.GetClassName(h) == "Tauri Window":
            out.append(win32gui.GetWindowRect(h))
    win32gui.EnumWindows(cb, None)
    return out

if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "shot":
        shot(sys.argv[2])
    elif cmd == "rect":
        print("Tauri rects:", var_rect())
    elif cmd == "wins":
        for r in enum_var_windows():
            print(r)
    elif cmd == "click":
        click(int(sys.argv[2]), int(sys.argv[3]), sys.argv[4] if len(sys.argv) > 4 else "")
        if len(sys.argv) > 5:
            shot(sys.argv[5])
    elif cmd == "dclick":
        dclick(int(sys.argv[2]), int(sys.argv[3]), sys.argv[4] if len(sys.argv) > 4 else "")
        if len(sys.argv) > 5:
            shot(sys.argv[5])
    elif cmd == "rclick":
        rclick(int(sys.argv[2]), int(sys.argv[3]), sys.argv[4] if len(sys.argv) > 4 else "")
        if len(sys.argv) > 5:
            shot(sys.argv[5])
    elif cmd == "key":
        key(*sys.argv[2:])
        shot("after_key")
    elif cmd == "type":
        typewrite(sys.argv[2])
        if len(sys.argv) > 3:
            shot(sys.argv[3])
