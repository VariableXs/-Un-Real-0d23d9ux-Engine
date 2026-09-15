# R3 Phase J: open settings via 个性化, then tour all tabs
import ctypes
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

# click 个性化 in desktop context menu
pyautogui.click(1296, 951); time.sleep(1.6)
s("r3_120_settings_open")
print("done")
