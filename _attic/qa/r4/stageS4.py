# R4 Stage S4：关 Windows 菜单 → Variable 桌面确认 → Steam 红钮关闭（干净退出）
from r4helper import *
import time

pyautogui.press("win")
time.sleep(1.2)
shot("r4_050_back_to_var")
# Steam 虚拟窗红绿灯：绿色关闭钮在 (display 658,90)→物理(1170,160)
click(1170, 160, "Steam窗关闭钮", wait=2.5)
shot("r4_051_steam_closed")
