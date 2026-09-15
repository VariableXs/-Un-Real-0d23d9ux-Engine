# R4 Stage E2：新建文件夹（工具栏）→ Enter 提交 → 删除 → 确认对话框 Enter
from r4helper import *
import time

click(960, 620, "关闭右键菜单", wait=0.8)
click(775, 68, "工具栏新建", wait=1.0)
shot("r4_022_newdlg")
pyautogui.press("shift")
typewrite("R4QA_Temp", wait=0.3)
shot("r4_023_named")
press("enter", wait=1.2)          # R2-B2 回归：Enter 应提交
shot("r4_024_created")
