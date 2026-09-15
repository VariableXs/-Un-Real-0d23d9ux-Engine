# R3 QA: launch variable.exe with stdout logging, verify boot
import ctypes, os, subprocess, sys, time
try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:
    ctypes.windll.user32.SetProcessDPIAware()
import pyautogui
pyautogui.FAILSAFE = False
pyautogui.PAUSE = 0.15

BASE = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main"
QA = BASE + r"/_attic/qa"
EXE = BASE + r"/src-tauri/target/release/variable.exe"
LOG = QA + r"/r3_run1.log"

# already running?
import ctypes.wintypes as wt
def find_win():
    res = []
    @ctypes.WINFUNCTYPE(ctypes.c_bool, wt.HWND, wt.LPARAM)
    def cb(h, l):
        buf = ctypes.create_unicode_buffer(256)
        ctypes.windll.user32.GetClassNameW(h, buf, 256)
        if buf.value == "Tauri Window":
            res.append(h)
        return True
    ctypes.windll.user32.EnumWindows(cb, 0)
    return res

existing = find_win()
if existing:
    print("ALREADY_RUNNING", existing)
else:
    logf = open(LOG, "w", encoding="utf-8", errors="replace")
    env = dict(os.environ)
    env["RUST_LOG"] = "info"
    p = subprocess.Popen([EXE], stdout=logf, stderr=subprocess.STDOUT,
                         cwd=BASE, env=env, creationflags=subprocess.CREATE_NO_WINDOW if False else 0)
    print("launched pid", p.pid)
    time.sleep(14)

print("screen size:", pyautogui.screenshot().size)
time.sleep(1)
pyautogui.screenshot().save(QA + r"/r3_001_boot.png")
print("saved r3_001_boot.png")
time.sleep(2)
pyautogui.screenshot().save(QA + r"/r3_002_boot2.png")
print("saved r3_002_boot2.png")
print("tauri windows now:", find_win())
