# M3 acceptance: type into embedded notepad (native focus path, no forwarding) and screenshot
import ctypes
try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:
    ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time, sys
pyautogui.FAILSAFE = False
SHOT = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"

# ascii-only: avoid IME issues
text = "M3 native path test 12345"
time.sleep(0.5)
t0 = time.perf_counter()
pyautogui.typewrite(text, interval=0.02)
dt = time.perf_counter() - t0
print(f"typed {len(text)} chars in {dt:.2f}s")
time.sleep(0.6)
p = pyautogui.screenshot()
p.save(f"{SHOT}/{sys.argv[1] if len(sys.argv)>1 else 'm3_typed'}.png")
print("saved")
