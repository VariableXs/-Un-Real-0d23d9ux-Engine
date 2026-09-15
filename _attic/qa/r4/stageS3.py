# R4 Stage S3：Win 键 → desktop_raise（WebView 提到 Steam 之上）→ 开始菜单
from r4helper import *
import time

pyautogui.press("win", _pause=False) if hasattr(pyautogui, "press") else None
pyautogui.press("win")
print("pressed win")
time.sleep(1.5)
shot("r4_049_winkey_raise")
