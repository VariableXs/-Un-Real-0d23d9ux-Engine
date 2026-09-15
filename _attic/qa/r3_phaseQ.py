# R3 Phase Q: close code-analysis view immediately (excluded app), use search for taskmgr
import ctypes
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

# close 项目分析 window immediately (excluded app, not inspected)
pyautogui.click(1653, 84); time.sleep(1.5)
s("r3_190_code_closed")
# open start menu, search "task"
pyautogui.click(759, 1041); time.sleep(1.2)
pyautogui.click(955, 94); time.sleep(0.5)
pyautogui.typewrite("task", interval=0.08); time.sleep(1.2)
s("r3_191_task_search")
pyautogui.click(500, 396); time.sleep(2.2)
s("r3_192_taskmgr_open")
print("done")
