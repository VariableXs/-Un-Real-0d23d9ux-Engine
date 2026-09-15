# R4 Stage B0：误开的内置窗（Write/Fate）仅做关闭（不进其内部任何功能）
from r4helper import *
import time

# Fate 窗在顶层：点标题栏聚焦 → Ctrl+W 关闭（R3-B3 修复路径，仅关闭动作）
click(700, 103, "Fate标题栏聚焦", wait=0.6)
key("ctrl", "w", wait=1.2)
shot("r4_013_after_close1")
# 若还有内置窗（Write），同样处理：点其标题栏再 Ctrl+W
click(500, 103, "下一窗标题栏", wait=0.6)
key("ctrl", "w", wait=1.2)
shot("r4_014_after_close2")
