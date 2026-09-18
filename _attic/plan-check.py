#!/usr/bin/env python3
"""布局预演：用定版配置对近似 1T 实盘（953GiB）跑纯计算，不碰磁盘。"""
import subprocess

PS = r"""
& 'D:\2\14\-Un-Real-0d23d9ux-Engine-main\portable\AI-P\Create-Partitions.ps1' -PlanOnly -PlanDiskGB 953 -ReportPath 'D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\plan-953.txt'
"PLAN_EXIT=$LASTEXITCODE"
"""

r = subprocess.run(["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", PS],
                   capture_output=True)
print(r.stdout.decode("gbk", "replace"))
err = r.stderr.decode("gbk", "replace")
if err.strip():
    print("[stderr]", err)
