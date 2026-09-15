# R4 Stage D：Del+Backspace 输入聚焦豁免（R2-S1 修复）+ 新建/删除对话框 Enter（R2-B2 回归）
from r4helper import *
import time

# 1) 输入聚焦时按 Del+Backspace → 应被豁免（不退出）
click(734, 1050, "V按钮", wait=1.2)
click(420, 53, "搜索框聚焦", wait=0.8)
key("delete", "backspace", wait=1.5)   # pyautogui 顺序按下，重叠窗口内被 kbdhook 判为同按
shot("r4_019_delback_exempt")
print("ALIVE_CHECK_1")
# 2) 打开资源管理器
click(42, 55, "桌面图标-文件管理器", wait=1.5)
dclick(42, 55, "文件管理器双击打开", wait=1.8)
shot("r4_020_explorer2")
