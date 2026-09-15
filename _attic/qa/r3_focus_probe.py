# R3: probe focus/foreground after clicking embedded steam field
import ctypes, ctypes.wintypes as wt
try: ctypes.windll.shcore.SetProcessDPIAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
u32 = ctypes.windll.user32; k32 = ctypes.windll.kernel32

def exe_of(pid):
    h = k32.OpenProcess(0x1000, False, pid)
    if h:
        buf = ctypes.create_unicode_buffer(512); sz = wt.DWORD(512)
        k32.QueryFullProcessImageNameW(h, 0, buf, ctypes.byref(sz)); k32.CloseHandle(h)
        return buf.value.split("\\")[-1]
    return "?"

def info(tag):
    fg = u32.GetForegroundWindow()
    pid = wt.DWORD(); u32.GetWindowThreadProcessId(fg, ctypes.byref(pid))
    buf = ctypes.create_unicode_buffer(128); u32.GetClassNameW(fg, buf, 128)
    print(tag, "FG hwnd=", fg, "cls=", buf.value, "pid=", pid.value, "exe=", exe_of(pid.value))
    # also get focused window of fg thread
    tid = u32.GetWindowThreadProcessId(fg, None)
    gf = u32.GetFocus()  # only valid for calling thread; use AttachThreadInput trick
    print(tag, "GetFocus(self)=", gf)

info("before")
pyautogui.click(787, 428); time.sleep(1.0)
info("after-click")
time.sleep(1.0)
info("after-2s")
