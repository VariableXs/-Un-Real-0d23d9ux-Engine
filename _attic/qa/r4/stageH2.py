# R4 Stage H2：关设置 → 时钟精确重试 → 空间面板精确重试
from r4helper import *
import time

key("ctrl", ",", wait=1.0)   # 切回设置（若已关则无效）
click(1605, 20, "设置×关闭", wait=1.2)
shot("r4_038_settings_closed")
# 时钟热区（display 1030..1060,562..575 → 物理取中）
click(1861, 1012, "时钟精确点", wait=1.3)
shot("r4_039_clock2")
click(960, 400, "外点关闭", wait=0.8)
# 空间面板按钮：display (410,568) → 物理 (729,1010) 已试无效；改试 (742,1010) 与 (715,1010)
click(742, 1010, "空间面板b", wait=1.2)
shot("r4_040_space2")
