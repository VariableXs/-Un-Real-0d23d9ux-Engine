# R3-B11 fix test A3 (close via red button): screenshot first, then click red button
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
s("rt2_close_before")   # calibrate red button position from this shot
print("STOP: inspect rt2_close_before for red button position, then run a3b", flush=True)
print("=== LOG MARK set ===", flush=True)
