# R3-B11 fix test A2b (cold start, pid-precise foreground): find variable.exe main window by pid
import ctypes, time, os, glob, subprocess
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

# 1) variable.exe pid
r = subprocess.run(["tasklist", "/FI", "IMAGENAME eq variable.exe", "/FO", "CSV"], capture_output=True)
line = [l for l in r.stdout.decode("gbk", "replace").splitlines() if l.lower().startswith('"variable.exe"')]
assert line, "variable.exe not running"
pid = int(line[0].split('","')[1])
print("variable pid", pid, flush=True)

# 2) enum top-level windows of that pid, visible + class "Tauri Window"
user32 = ctypes.windll.user32
EnumWindows = user32.EnumWindows
found = []
WNDENUMPROC = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_void_p)
def _cb(hwnd, _lp):
    if not user32.IsWindowVisible(hwnd): return True
    wpid = ctypes.c_uint(0)
    user32.GetWindowThreadProcessId(hwnd, ctypes.byref(wpid))
    if wpid.value != pid: return True
    buf = ctypes.create_unicode_buffer(64)
    user32.GetClassNameW(hwnd, buf, 64)
    if buf.value == "Tauri Window": found.append(hwnd)
    return True
EnumWindows(WNDENUMPROC(_cb), None)
assert found, "variable main window not found"
u32.SetForegroundWindow(found[0]); time.sleep(1.2)
s("rt3_foreground")   # verify desktop is in front BEFORE clicking

mark = len(log_snapshot())
pyautogui.doubleClick(75, 923); print("double-clicked desktop steam icon", flush=True)
for i in range(14):
    time.sleep(2)
    s(f"rt3_cold_{i:02d}")
delta = log_snapshot()[mark:]
print("=== LOG DELTA (cold2) ===", flush=True)
print(delta[-3000:] if delta else "(no new log)", flush=True)
