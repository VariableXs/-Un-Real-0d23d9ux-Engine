# R3 Phase O: close placeholder, toggle calendar, open task manager cleanly
import ctypes
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

# close windowsterminal placeholder
pyautogui.click(1031, 583); time.sleep(1.2)
s("r3_170_placeholder_closed")
# toggle calendar via clock again
pyautogui.click(1872, 1036); time.sleep(1.0)
s("r3_171_cal_toggle2")
# open start menu and click task manager
pyautogui.click(759, 1041); time.sleep(1.2)
s("r3_172_startmenu_state")
pyautogui.click(528, 579); time.sleep(2.2)
s("r3_173_taskmgr2")
print("done")
