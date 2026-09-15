# R3 Phase E: steam wheel/right-click/close tests
import ctypes
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

# 1. wheel scroll inside steam area
pyautogui.moveTo(787, 500)
pyautogui.scroll(-3); time.sleep(0.8)
pyautogui.scroll(3); time.sleep(0.8)
s("r3_060_steam_wheel")
# 2. right click inside steam
pyautogui.click(787, 428, button="right"); time.sleep(1.0)
s("r3_061_steam_rightclick")
pyautogui.press("esc"); time.sleep(0.5)
# 3. close VWM session via X button
pyautogui.click(1730, 178); time.sleep(2.0)
s("r3_062_after_close")
print("done")
