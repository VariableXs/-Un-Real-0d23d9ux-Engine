# -*- coding: utf-8 -*-
"""AI-2 · W1 部署专用监视器 v3：只盯日志终态（DONE/FAIL），2 小时窗口。"""
import sys
import time

LOG = r"D:\VarixDeploy\w1-deploy.log"
deadline = time.time() + 120 * 60
last_len = 0
while time.time() < deadline:
    try:
        data = open(LOG, "rb").read().decode("utf-8-sig", "replace")
    except FileNotFoundError:
        data = ""
    if len(data) != last_len:
        last_len = len(data)
        lines = [l for l in data.strip().splitlines() if l.startswith("[")]
        print("[log] " + (lines[-1] if lines else "(empty)"), flush=True)
    if "W1-DEPLOY-DONE" in data:
        print("=== W1-DEPLOY-DONE ===", flush=True)
        sys.stdout.write(data[-1200:])
        sys.exit(0)
    if "W1-DEPLOY-FAIL" in data:
        print("=== W1-DEPLOY-FAIL ===", flush=True)
        sys.stdout.write(data[-1500:])
        sys.exit(1)
    time.sleep(20)
print("=== MONITOR TIMEOUT 120min ===")
sys.exit(2)
