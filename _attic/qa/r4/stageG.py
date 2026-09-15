# R4 Stage G：删除 R4QA_Temp → 确认对话框 Enter（R2-B2 回归）→ toast/角标
from r4helper import *
import time

click(889, 249, "选中R4QA_Temp行", wait=0.8)
press("delete", wait=1.2)
shot("r4_032_delconf")     # 确认对话框应弹出
press("enter", wait=1.4)   # Enter 应提交删除
shot("r4_033_deleted")     # 行消失 + toast + 回收站角标+1
