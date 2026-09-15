# R3-B4 step13: menu -> search 设置 -> screenshot results for calibration
import ctypes, time
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
u32 = ctypes.windll.user32; k32 = ctypes.windll.kernel32
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n, flush=True)

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

u32.SetForegroundWindow(u32.FindWindowW("Tauri Window", None)); time.sleep(1.0)
pyautogui.click(759, 1041); time.sleep(2.0)     # V button - start menu
s("rv_T1_menu")
pyautogui.click(959, 97); time.sleep(1.0)       # search box
set_clipboard("设置")
pyautogui.hotkey("ctrl", "v"); time.sleep(1.8)
s("rv_T2_results")
print("step13 done", flush=True)
