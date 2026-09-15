# R4 Stage C：Ctrl+W 关闭虚拟窗回归（R3-B3 修复）+ 资源管理器流程 + Del+Backspace 输入豁免
# 纪律：全程不用 Esc（避免与全局 double-Esc 冲突）；不碰四款内置软件
from r4helper import *
import time

# 1) 开始菜单 → 搜索 explorer → Enter 打开文件管理器
click(734, 1050, "V按钮", wait=1.2)
click(420, 53, "搜索框", wait=0.8)
pyautogui.press("shift")
typewrite("explorer", wait=0.4)
press("enter", wait=1.6)
shot("r4_017_explorer_open")
# 2) 点文件列表空白区（确保焦点在窗体而非输入框）
click(760, 430, "列表区聚焦", wait=0.5)
# 3) Ctrl+W → 应只关当前虚拟窗，variable 存活
key("ctrl", "w", wait=1.5)
shot("r4_018_after_ctrlw")
print("PROC_CHECK")
