# R3 Phase D: input routing test into embedded Steam + close test
import ctypes
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

# 1. click Steam username field (embedded window)
pyautogui.click(787, 428); time.sleep(0.8)
s("r3_050_steam_user_focus")
# 2. type ASCII text
pyautogui.typewrite("VARIABLE_QA_R3", interval=0.05); time.sleep(1.0)
s("r3_051_steam_typed")
# 3. click password field, type
pyautogui.click(787, 528); time.sleep(0.6)
pyautogui.typewrite("testpwd123", interval=0.05); time.sleep(1.0)
s("r3_052_steam_pwd")
# 4. remember-password checkbox toggle
pyautogui.click(395, 622); time.sleep(0.6)
s("r3_053_steam_checkbox")
print("done")
