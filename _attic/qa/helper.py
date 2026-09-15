# QA helper for Variable system GUI testing
import ctypes, sys
try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)  # per-monitor DPI aware
except Exception:
    ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False
pyautogui.PAUSE = 0.35
SHOT = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"

def shot(name):
    p = pyautogui.screenshot()
    p.save(f"{SHOT}/{name}.png")
    print("saved", name, p.size)

def click(x, y, name=""):
    # coords given in 1080-wide display space, screen is 1920 wide
    sx, sy = x * 1920 / 1080, y * 1080 / 608
    pyautogui.click(sx, sy)
    print(f"clicked {name} at ({x},{y})->({sx:.0f},{sy:.0f})")
    time.sleep(0.8)

def key(*k):
    pyautogui.hotkey(*k)
    print("key", k)
    time.sleep(0.8)

if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "shot":
        shot(sys.argv[2])
    elif cmd == "click":
        click(int(sys.argv[2]), int(sys.argv[3]), sys.argv[4] if len(sys.argv) > 4 else "")
        if len(sys.argv) > 5:
            shot(sys.argv[5])
    elif cmd == "key":
        key(*sys.argv[2:])
        shot("after_key")
    elif cmd == "type":
        pyautogui.typewrite(sys.argv[2], interval=0.03)
