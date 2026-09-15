# R3 Phase A2: state shot, dismiss toast, open start menu
import ctypes
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

s("r3_020_state")
# dismiss WE toast if present
pyautogui.click(788, 206); time.sleep(1.0); s("r3_021_toast_dismissed")
# open start menu via V button
pyautogui.click(759, 1041); time.sleep(1.3); s("r3_022_startmenu")
print("done")
