# R4 Stage H3：时钟/空间面板点击下移（避开 WE 提示条吞层）
from r4helper import *
import time

click(1861, 1062, "时钟下移点", wait=1.3)
shot("r4_041_clock3")
click(960, 500, "外点关闭", wait=0.8)
click(742, 1062, "空间面板下移点", wait=1.3)
shot("r4_042_space3")
