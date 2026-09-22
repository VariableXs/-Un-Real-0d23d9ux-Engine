# -*- coding: utf-8 -*-
"""提权执行 vx-wincrash.py（UAC 自举）。"""
import os, sys, time, ctypes

HERE = os.path.dirname(os.path.abspath(__file__))
TARGET = os.path.join(HERE, "vx-wincrash.py")
LOG = os.path.join(HERE, "vx-wincrash.rpt")

if os.path.exists(LOG):
    os.remove(LOG)

rc = ctypes.windll.shell32.ShellExecuteW(
    None, "runas", sys.executable, f'"{TARGET}"', None, 0)
print("ShellExecute rc=", rc, "(请点 UAC「是」)")
for _ in range(150):
    time.sleep(2)
    if os.path.exists(LOG):
        print(open(LOG, encoding="utf-8").read())
        break
else:
    print("timeout waiting report")
