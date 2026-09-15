# R4 Stage A：警告浮窗关闭 / 开始菜单 / 搜索链路（R2-B5 回归）/ 搜索残留（R3-B6 修复验证）
from r4helper import *
import time

shot("r4_002_with_warn")
click(443, 117, "warn-知道了", wait=1.0)          # 关闭 Wallpaper Engine 警告浮窗
shot("r4_003_warn_closed")
click(734, 1050, "V按钮", wait=1.2)               # 打开开始菜单
shot("r4_004_startmenu")
click(960, 1000, "搜索框", wait=0.6)              # 底部搜索框
pyautogui.press("shift")                           # 确保英文态
typewrite("explorer", wait=0.4)
shot("r4_005_typed")
press("enter", wait=1.2)
shot("r4_006_search_overlay")                      # R2-B5 回归：查询词应带入浮层
key("esc", wait=0.8)
click(734, 1050, "V按钮-再开", wait=1.2)
shot("r4_007_reopen_empty")                        # R3-B6 修复验证：搜索框应为空
click(960, 300, "菜单空白处关菜单", wait=0.6)
