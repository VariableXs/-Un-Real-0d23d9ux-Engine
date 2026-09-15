# R3-B1 step6: feasibility probe - AttachThreadInput + SetFocus + SendInput into steam
import ctypes, time
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
u32 = ctypes.windll.user32; k32 = ctypes.windll.kernel32
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n, flush=True)

HWND = 1573622
HWND_VAR = u32.FindWindowW("Tauri Window", None)

class KEYBDINPUT(ctypes.Structure):
    _fields_ = [("wVk", ctypes.c_ushort), ("wScan", ctypes.c_ushort),
                ("dwFlags", ctypes.c_ulong), ("time", ctypes.c_ulong),
                ("dwExtraInfo", ctypes.POINTER(ctypes.c_ulong))]
class INPUT(ctypes.Structure):
    class _U(ctypes.Union):
        _fields_ = [("ki", KEYBDINPUT)]
    _anonymous_ = ("u",)
    _fields_ = [("type", ctypes.c_ulong), ("u", _U)]

def tap_vk(vk):
    for flags in (0, 2):  # down, up(KEYEVENTF_KEYUP)
        inp = INPUT(type=1); inp.ki = KEYBDINPUT(vk, 0, flags, 0, None)
        u32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(INPUT))
        time.sleep(0.03)

u32.SetForegroundWindow(HWND_VAR); time.sleep(1.0)
tid_steam = u32.GetWindowThreadProcessId(HWND, None)
tid_me = k32.GetCurrentThreadId()
ok = u32.AttachThreadInput(tid_me, tid_steam, True)
print("attach:", ok, "tid_steam:", tid_steam, flush=True)
u32.SetFocus(HWND); time.sleep(0.3)
for vk in (ord('A'), ord('B')):
    tap_vk(vk); time.sleep(0.15)
u32.AttachThreadInput(tid_me, tid_steam, False)
u32.SetForegroundWindow(HWND_VAR); time.sleep(1.2)
s("rv_J1_attachinput_AB")
print("step6 done", flush=True)
