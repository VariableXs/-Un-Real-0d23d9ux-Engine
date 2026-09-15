# R4 Stage A2：搜索链路重试（正确坐标）
from r4helper import *
import time

click(960, 300, "关菜单", wait=0.8)
click(734, 1050, "V按钮", wait=1.2)
click(420, 53, "搜索框(顶部)", wait=0.8)
pyautogui.press("shift")
typewrite("explorer", wait=0.4)
shot("r4_008_typed2")
press("enter", wait=1.4)
shot("r4_009_overlay2")          # R2-B5 回归：浮层应带入 "explorer"
click(960, 1040, "关闭浮层/落焦点", wait=0.6)
key("esc", wait=0.6)
click(734, 1050, "V按钮-再开", wait=1.2)
shot("r4_010_reopen_check")      # R3-B6 修复验证：搜索框应为空
