# R3-B1 reverify step1: open menu -> focus search -> type steam -> STOP for calibration
import ctypes, time
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
u32 = ctypes.windll.user32
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n, flush=True)

u32.SetForegroundWindow(u32.FindWindowW("Tauri Window", None)); time.sleep(0.8)
pyautogui.press("esc"); time.sleep(0.6)   # clear any leftover popup
pyautogui.click(759, 1041); time.sleep(1.8)   # open start menu, wait full animation
s("rv_E1_menu_open")
pyautogui.click(959, 97); time.sleep(1.0)     # focus search box
s("rv_E2_search_focus")
pyautogui.typewrite("steam", interval=0.09); time.sleep(1.6)
s("rv_E3_typed")
print("step1 done - inspect rv_E3 for filtered results", flush=True)
