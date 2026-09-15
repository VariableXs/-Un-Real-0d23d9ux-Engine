# R4 Stage S：Steam 独立收编测试（L1 真实嵌入）+ Win 键回桌面（R4-B6 验证）
from r4helper import *
import time

click(960, 300, "关开始菜单", wait=0.8)
dclick(237, 210, "桌面Steam图标", wait=6.0)   # 冷启动等待
shot("r4_043_steam1")
time.sleep(6)
shot("r4_044_steam2")
