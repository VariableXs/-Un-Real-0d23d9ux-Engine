# R3-B1 step7: SetForegroundWindow(steam) + SendInput + restore Variable foreground
import ctypes, time
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
u32 = ctypes.windll.user32
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
    for flags in (0, 2):
        inp = INPUT(type=1); inp.ki = KEYBDINPUT(vk, 0, flags, 0, None)
        u32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(INPUT))
        time.sleep(0.03)

u32.SetForegroundWindow(HWND_VAR); time.sleep(0.8)
u32.SetForegroundWindow(HWND); time.sleep(0.8)   # steam becomes real foreground (offscreen)
fg = u32.GetForegroundWindow()
print("fg hwnd:", fg, "expect", HWND, flush=True)
for vk in (ord('A'), ord('B'), ord('C')):
    tap_vk(vk); time.sleep(0.12)
u32.SetForegroundWindow(HWND_VAR); time.sleep(1.4)  # restore
s("rv_K1_fg_sendinput_ABC")
print("step7 done", flush=True)
