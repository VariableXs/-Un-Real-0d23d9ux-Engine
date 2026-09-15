# R3 Phase R: properly close project-analysis, open taskmgr via yesterday chip
import ctypes
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

# close 项目分析 via red light (corrected y)
pyautogui.click(1646, 44); time.sleep(1.5)
s("r3_200_code_closed2")
# clear start menu search
pyautogui.click(1467, 94); time.sleep(0.8)
# click 任务管理器 chip in 昨日接触
pyautogui.click(466, 187); time.sleep(2.2)
s("r3_201_taskmgr_final")
print("done")
