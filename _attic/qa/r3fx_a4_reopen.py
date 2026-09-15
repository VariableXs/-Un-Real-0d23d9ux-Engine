# R3-B11 fix test A4 (singleton relaunch): double-click desktop Steam icon again
import ctypes, time, os, glob
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.25
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
LOGDIR = os.environ["APPDATA"] + r"/com.variable.app/logs"
u32 = ctypes.windll.user32
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n, flush=True)
def log_snapshot():
    fs = sorted(glob.glob(LOGDIR + "/applog-*.log"), key=os.path.getmtime)
    return open(fs[-1], "rb").read().decode("utf-8", "replace") if fs else ""

mark = len(log_snapshot())
u32.SetForegroundWindow(u32.FindWindowW("Tauri Window", None)); time.sleep(0.8)
pyautogui.press("esc"); time.sleep(0.6)
pyautogui.doubleClick(75, 923); print("double-clicked desktop steam icon (relaunch)", flush=True)
for i in range(10):
    time.sleep(2)
    s(f"rt2_reopen_{i:02d}")
cur = log_snapshot()
delta = cur[mark:]
print("=== LOG DELTA (relaunch) ===", flush=True)
print(delta[-3000:] if delta else "(no new log)", flush=True)
