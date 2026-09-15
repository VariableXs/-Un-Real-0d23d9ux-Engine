# R3 Phase U: chinese search (datavault) on restarted variable + volume slider drag
import ctypes, time
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

def send_unicode(text):
    import ctypes.wintypes as wt
    class KI(ctypes.Structure):
        _fields_ = [("wVk", wt.WORD), ("wScan", wt.WORD), ("dwFlags", wt.DWORD),
                    ("time", wt.DWORD), ("dwExtraInfo", ctypes.POINTER(ctypes.c_ulong))]
    class INPUT(ctypes.Structure):
        class _U(ctypes.Union):
            _fields_ = [("ki", KI), ("pad", ctypes.c_byte * 24)]
        _anonymous_ = ("u",); _fields_ = [("type", wt.DWORD), ("u", _U)]
    u32 = ctypes.windll.user32
    for ch in text:
        i1 = INPUT(type=1); i1.ki = KI(0, ord(ch), 0x0004, 0, None)
        i2 = INPUT(type=1); i2.ki = KI(0, ord(ch), 0x0006, 0, None)
        u32.SendInput(1, ctypes.byref(i1), ctypes.sizeof(INPUT))
        u32.SendInput(1, ctypes.byref(i2), ctypes.sizeof(INPUT))
        time.sleep(0.03)

time.sleep(3)  # let boot settle
s("r3_240_restarted")
# open start menu, search 数据金库
pyautogui.click(759, 1041); time.sleep(1.3)
pyautogui.click(955, 94); time.sleep(0.6)
send_unicode("数据金库")
time.sleep(1.3)
s("r3_241_cn_search")
# click first result if any
pyautogui.click(500, 396); time.sleep(2.2)
s("r3_242_vault_open")
print("done")
