# R3 Phase P: clear search (menu is open now), open task manager
import ctypes
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

pyautogui.click(1467, 94); time.sleep(1.0)
s("r3_180_cleared")
pyautogui.click(528, 579); time.sleep(2.2)
s("r3_181_taskmgr3")
print("done")
