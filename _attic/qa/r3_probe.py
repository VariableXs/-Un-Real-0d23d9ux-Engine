# R3 probe: close windows start menu, bring Variable to front, measure window rect
import ctypes
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
u32 = ctypes.windll.user32

def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

# 1. close windows start menu
pyautogui.press("esc"); time.sleep(0.8)

# 2. find Variable window + rect
rect = ctypes.wintypes.RECT() if hasattr(ctypes, "wintypes") else None
import ctypes.wintypes as wt
r = wt.RECT()
found = []
@ctypes.WINFUNCTYPE(ctypes.c_bool, wt.HWND, wt.LPARAM)
def cb(h, l):
    buf = ctypes.create_unicode_buffer(256)
    u32.GetClassNameW(h, buf, 256)
    if buf.value == "Tauri Window":
        found.append(h)
    return True
u32.EnumWindows(cb, 0)
print("tauri hwnds:", found)
for h in found:
    u32.GetWindowRect(h, ctypes.byref(r))
    print("hwnd", h, "rect", r.left, r.top, r.right, r.bottom)
    # foreground it
    u32.SetForegroundWindow(h)
    time.sleep(0.5)
s("r3_013_var_fore")
# 3. re-measure after foreground
for h in found:
    u32.GetWindowRect(h, ctypes.byref(r))
    print("after fg rect", r.left, r.top, r.right, r.bottom)
