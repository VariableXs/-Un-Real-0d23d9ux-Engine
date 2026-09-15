# R4 Stage F3：物理坐标修正后重做（显示坐标 ×1.778 = 物理坐标）
from r4helper import *
import time

click(467, 155, "侧栏-主目录(物理)", wait=1.5)
shot("r4_030_home_phys")
click(1529, 121, "搜索框(物理)", wait=0.7)
pyautogui.press("shift")
typewrite("R4QA", wait=0.6)
press("enter", wait=1.5)
shot("r4_031_searchR4QA")
