# R3 Phase T: ctrl+w close attempt; then unicode-Chinese search for datavault
import ctypes, time
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

def send_unicode(text):
    # KEYEVENTF_UNICODE = 0x0004; KEYUP = 0x0002
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
        inp = INPUT(type=1); inp.ki = KI(0, ord(ch), 0x0004, 0, None)
        inp2 = INPUT(type=1); inp2.ki = KI(0, ord(ch), 0x0006, 0, None)
        u32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(INPUT))
        u32.SendInput(1, ctypes.byref(inp2), ctypes.sizeof(INPUT))
        time.sleep(0.03)

# 1. focus window, ctrl+w
pyautogui.click(356, 92); time.sleep(0.8)
pyautogui.hotkey("ctrl", "w"); time.sleep(1.2)
s("r3_230_ctrlw")

# 2. start menu search with Chinese
pyautogui.click(759, 1041); time.sleep(1.2)
pyautogui.click(955, 94); time.sleep(0.6)
# clear existing query via ctrl+a delete
pyautogui.hotkey("ctrl", "a"); time.sleep(0.3)
pyautogui.press("delete"); time.sleep(0.5)
send_unicode("数据金库")
time.sleep(1.2)
s("r3_231_cn_search")
print("done")
