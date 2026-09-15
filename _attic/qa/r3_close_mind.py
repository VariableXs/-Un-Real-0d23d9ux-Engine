# close accidentally-opened Variable Mind window (not inspected)
import ctypes
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
import pyautogui, time
pyautogui.FAILSAFE = False; pyautogui.PAUSE = 0.3
pyautogui.click(1619, 48); time.sleep(1.5)
pyautogui.screenshot().save(r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/_attic/qa/r3_090_mind_closed.png")
print("closed")
