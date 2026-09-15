# R3 Phase S: focus project-analysis, look for lights, then Alt+F4
import ctypes
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
QA = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa"
def s(n): pyautogui.screenshot().save(QA + "/" + n + ".png"); print("shot", n)

pyautogui.click(356, 92); time.sleep(1.0)
s("r3_210_pa_focused")
pyautogui.moveTo(1680, 50); time.sleep(0.8)
s("r3_211_pa_hover_tr")
pyautogui.hotkey("alt", "f4"); time.sleep(1.5)
s("r3_212_pa_after_altf4")
print("done")
