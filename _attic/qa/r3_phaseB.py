# R3 Phase B: search "steam" in Variable start menu
import ctypes
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

# click search box (physical coords)
pyautogui.click(955, 94); time.sleep(0.6)
pyautogui.typewrite("steam", interval=0.08); time.sleep(1.0)
s("r3_030_typed_steam")
# live results should appear; screenshot then Enter
pyautogui.press("enter"); time.sleep(1.5)
s("r3_031_after_enter")
print("done")
