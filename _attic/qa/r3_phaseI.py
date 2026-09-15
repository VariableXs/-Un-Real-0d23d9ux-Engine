# R3 Phase I: quick panel toggles + volume + taskbar right-click menu
import ctypes
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

# quick panel is open; toggle 勿扰模式
pyautogui.click(1622, 233); time.sleep(0.9)
s("r3_110_dnd_on")
pyautogui.click(1622, 233); time.sleep(0.9)
s("r3_111_dnd_off")
# volume slider: click mid position
pyautogui.click(1600, 539); time.sleep(0.8)
s("r3_112_volume_set")
# close quick panel
pyautogui.click(1749, 169); time.sleep(0.9)
s("r3_113_qp_closed")
# right-click taskbar empty area
pyautogui.click(1244, 1041, button="right"); time.sleep(1.0)
s("r3_114_tb_ctx")
print("done")
