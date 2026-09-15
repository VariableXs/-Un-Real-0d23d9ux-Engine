# R3 Phase C: click Steam search result -> launch inside Variable
import ctypes
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

# click Steam result row
pyautogui.click(500, 396); time.sleep(6)
s("r3_040_steam_launching")
time.sleep(10)
s("r3_041_steam_launched")
print("done")
