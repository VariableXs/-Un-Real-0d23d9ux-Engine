# R4 Stage F2：重试侧栏导航 + 搜索定位 R4QA_Temp
from r4helper import *
import time

click(265, 86, "侧栏主目录", wait=1.5)
shot("r4_027_home2")
# 用搜索框过滤，避免滚动盲找
click(860, 68, "搜索框", wait=0.6)
pyautogui.press("shift")
typewrite("R4QA", wait=0.6)
shot("r4_028_searched")
press("enter", wait=1.2)
shot("r4_029_search_result")
