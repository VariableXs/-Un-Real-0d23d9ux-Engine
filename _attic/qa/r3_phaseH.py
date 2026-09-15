# R3 Phase H: clock popup + notification/quick panel
import ctypes
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

# 1. clock popup
pyautogui.click(1872, 1036); time.sleep(1.3)
s("r3_100_clock_popup")
# close it
pyautogui.press("esc"); time.sleep(0.8)
# 2. bell quick panel
pyautogui.click(1511, 1041); time.sleep(1.3)
s("r3_101_quickpanel")
print("done")
