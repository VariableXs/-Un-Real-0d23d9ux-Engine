# -*- coding: utf-8 -*-
"""AI-2 · W1 部署监视器 v4：按“最后一轮部署开始”锚定当前会话，忽略历史 FAIL 行。"""
import sys
import time

LOG = r"D:\VarixDeploy\w1-deploy.log"
deadline = time.time() + 150 * 60
last_session_len = -1
while time.time() < deadline:
    try:
        data = open(LOG, "rb").read().decode("utf-8-sig", "replace")
    except FileNotFoundError:
        data = ""
    anchor = data.rfind("W1 部署开始")
    session = data[anchor:] if anchor >= 0 else data
    if len(session) != last_session_len:
        last_session_len = len(session)
        lines = [l for l in session.strip().splitlines() if l.startswith("[")]
        print("[log] " + (lines[-1] if lines else "(empty)"), flush=True)
    if "W1-DEPLOY-DONE" in session:
        print("=== W1-DEPLOY-DONE ===", flush=True)
        sys.stdout.write(session[-1000:])
        sys.exit(0)
    if "W1-DEPLOY-FAIL" in session:
        print("=== W1-DEPLOY-FAIL ===", flush=True)
        sys.stdout.write(session[-1200:])
        sys.exit(1)
    time.sleep(20)
print("=== MONITOR TIMEOUT 150min ===")
sys.exit(2)
