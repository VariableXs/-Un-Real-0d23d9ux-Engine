# R4 Stage H：时钟日历弹层（R3-B2 回归）+ 空间面板滑块 + 设置中心
from r4helper import *
import time

# 1) 时钟 → 日历弹层 → 外点关闭（R3-B2 回归）
click(1849, 1010, "任务栏时钟", wait=1.2)
shot("r4_034_clockpopup")
click(960, 400, "桌面空白外点", wait=0.8)
shot("r4_035_clockclosed")
# 2) 空间面板：任务栏空间按钮（display 410,568 → 物理约 729,1010 附近先试）
click(729, 1010, "任务栏空间面板按钮", wait=1.2)
shot("r4_036_spacepanel")
# 3) 设置中心 Ctrl+,
key("ctrl", ",", wait=1.6)
shot("r4_037_settings")
