# R3-B1 step5: direct PostMessage WM_CHAR / WM_IME_CHAR into steam hwnd to isolate
import ctypes, time
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
u32 = ctypes.windll.user32
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n, flush=True)

HWND = 1573622  # steam main window from r3_run2.log embed_launch
WM_CHAR = 0x0102
WM_IME_CHAR = 0x0286

# ensure Variable foreground (steam embedded view visible)
u32.SetForegroundWindow(u32.FindWindowW("Tauri Window", None)); time.sleep(1.0)

# post a few chars: "QA"
for ch in "QA":
    u32.PostMessageW(HWND, WM_CHAR, ord(ch), 0); time.sleep(0.25)
time.sleep(1.2)
s("rv_I1_wmchar_QA")

# if nothing, try WM_IME_CHAR "Z"
u32.PostMessageW(HWND, WM_IME_CHAR, ord("Z"), 0); time.sleep(1.0)
s("rv_I2_imechar_Z")
print("step5 done", flush=True)
