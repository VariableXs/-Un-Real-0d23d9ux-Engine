# R4 Stage B1：资源管理器右键（B-1 回归）
from r4helper import *
import time

click(700, 300, "资源管理器列表行聚焦", wait=0.6)
rclick(700, 300, "列表行右键", wait=1.2)
shot("r4_011_ctxmenu")      # B-1 回归：shell 上下文菜单应弹出
press("esc", wait=0.6)
shot("r4_012_menu_closed")
