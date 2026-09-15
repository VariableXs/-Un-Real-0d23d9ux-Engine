# R3 Phase L: test maintenance actions (log rotate + VACUUM)
import ctypes
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

# go to 性能与维护 tab
pyautogui.click(514, 951); time.sleep(1.0)
# click 立即轮转清理日志
pyautogui.click(802, 845); time.sleep(1.2)
s("r3_140_log_rotate")
# click 数据库紧凑(VACUUM)
pyautogui.click(821, 956); time.sleep(1.5)
s("r3_141_vacuum")
print("done")
