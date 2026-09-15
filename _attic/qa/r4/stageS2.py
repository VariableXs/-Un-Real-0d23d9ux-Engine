# R4 Stage S2：开始菜单最近添加 → Steam chip → steam_launch 收编通道
from r4helper import *
import time

click(734, 1050, "V按钮", wait=1.2)
click(439, 573, "最近添加-Steam chip", wait=2.0)
shot("r4_047_steam_launch1")
time.sleep(10)
shot("r4_048_steam_launch2")
