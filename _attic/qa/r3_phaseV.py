# R3 Phase V: clipboard-paste Chinese search for 任务管理器 then 数据金库
import ctypes, time
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
u32 = ctypes.windll.user32; k32 = ctypes.windll.kernel32
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

def set_clipboard(text):
    CF_UNICODETEXT = 13; GMEM_MOVEABLE = 0x0002
    k32.GlobalAlloc.restype = ctypes.c_void_p
    k32.GlobalAlloc.argtypes = [ctypes.c_uint, ctypes.c_size_t]
    k32.GlobalLock.restype = ctypes.c_void_p
    k32.GlobalLock.argtypes = [ctypes.c_void_p]
    k32.GlobalUnlock.argtypes = [ctypes.c_void_p]
    u32.SetClipboardData.argtypes = [ctypes.c_uint, ctypes.c_void_p]
    h = k32.GlobalAlloc(GMEM_MOVEABLE, (len(text) + 1) * 2)
    p = k32.GlobalLock(h)
    ctypes.memmove(p, ctypes.create_unicode_buffer(text), (len(text) + 1) * 2)
    k32.GlobalUnlock(h)
    if u32.OpenClipboard(0):
        u32.EmptyClipboard(); u32.SetClipboardData(CF_UNICODETEXT, h); u32.CloseClipboard()

def search_cn(query):
    pyautogui.click(759, 1041); time.sleep(1.3)   # open start menu
    pyautogui.click(955, 94); time.sleep(0.6)     # focus search box
    set_clipboard(query)
    pyautogui.hotkey("ctrl", "v"); time.sleep(1.4)
    s("r3_27x_search_" + query)

search_cn("任务管理器")
# screenshot first, decide click target after seeing results
s("r3_272_state")
print("done")
