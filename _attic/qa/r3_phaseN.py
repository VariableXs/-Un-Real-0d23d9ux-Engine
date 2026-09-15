# R3 Phase N: toggle calendar via clock, clear search, open task manager
import ctypes
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

# toggle calendar closed via clock click
pyautogui.click(1872, 1036); time.sleep(1.0)
s("r3_160_cal_toggled")
# clear search box via its X
pyautogui.click(1467, 94); time.sleep(0.9)
s("r3_161_search_cleared")
# click 任务管理器 in unfiltered 最近使用
pyautogui.click(470, 579); time.sleep(2.0)
s("r3_162_taskmgr")
print("done")
