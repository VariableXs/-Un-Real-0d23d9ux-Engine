# -*- coding: utf-8 -*-
"""提权重跑 vx-diag-bootloop3.py（UAC 自举，等待报告落盘）。"""
import os, sys, time, ctypes

HERE = os.path.dirname(os.path.abspath(__file__))
TARGET = os.path.join(HERE, "vx-diag-bootloop3.py")
LOG = os.path.join(HERE, "vx-diag-bootloop3.rpt")

if os.path.exists(LOG):
    os.remove(LOG)

rc = ctypes.windll.shell32.ShellExecuteW(
    None, "runas", sys.executable, f'"{TARGET}"', None, 0)
print("ShellExecute rc=", rc)
for _ in range(120):
    time.sleep(2)
    if os.path.exists(LOG):
        print(open(LOG, encoding="utf-8").read())
        break
else:
    print("timeout waiting report")
