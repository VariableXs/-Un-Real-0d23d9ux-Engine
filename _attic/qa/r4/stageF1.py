# R4 Stage F：侧栏回主页 → 选中 R4QA_Temp → Delete → 确认框 Enter（R2-B2 回归）→ 回收站角标
from r4helper import *
import time

click(263, 87, "侧栏-主目录", wait=1.2)
shot("r4_025_home")
# 找到 R4QA_Temp 行：主页按名称排序，R 开头在后面，滚到底
scrolls = 14
for i in range(scrolls):
    pyautogui.scroll(-5, 700, 300)
    time.sleep(0.15)
shot("r4_026_scrolled")
