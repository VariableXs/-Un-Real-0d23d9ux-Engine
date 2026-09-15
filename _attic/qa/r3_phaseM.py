# R3 Phase M: close settings, open task manager via start menu
import ctypes
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

# close settings window (red traffic light)
pyautogui.click(1619, 20); time.sleep(1.2)
# open start menu
pyautogui.click(759, 1041); time.sleep(1.2)
# click 任务管理器 in 最近使用
pyautogui.click(470, 579); time.sleep(2.0)
s("r3_150_taskmgr")
print("done")
