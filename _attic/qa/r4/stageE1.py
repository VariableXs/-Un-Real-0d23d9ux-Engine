# R4 Stage E：资源管理器右键菜单（B-1 回归）→ 新建文件夹 → 对话框 Enter（R2-B2）
from r4helper import *
import time

rclick(700, 490, "列表空白区右键", wait=1.3)
shot("r4_021_ctxmenu2")     # shell 上下文菜单应出现
