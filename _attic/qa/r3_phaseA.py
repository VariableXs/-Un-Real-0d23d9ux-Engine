# R3 Phase A: dismiss WE toast, open start menu, screenshots
import ctypes
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"

def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

# 1. click 知道了 (toast dismiss)
pyautogui.click(788, 206); time.sleep(1.0); s("r3_010_toast_dismissed")
# 2. click V button (taskbar start)
pyautogui.click(759, 1041); time.sleep(1.2); s("r3_011_startmenu")
# 3. hover a start-menu card to observe hover state
pyautogui.moveTo(500, 400); time.sleep(0.8); s("r3_012_start_hover")
print("done")
