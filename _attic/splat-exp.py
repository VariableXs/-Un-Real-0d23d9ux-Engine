#!/usr/bin/env python3
"""实验：数组 splat 传参是否按预期绑定（-PlanOnly 在管理员门禁前，非提权可验）。"""
import subprocess

SCRIPT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\portable\AI-P\Create-Partitions.ps1"
PS = r"""
'--- EXP1: single-element array splat -PlanOnly ---'
& '{S}' @('-PlanOnly') 2>&1 | Select-Object -First 4
"EXP1_EXIT=$LASTEXITCODE"
'--- EXP2: multi-element array splat (-DiskNumber 1 -PlanOnly) ---'
& '{S}' @('-DiskNumber','1','-PlanOnly') 2>&1 | Select-Object -First 4
"EXP2_EXIT=$LASTEXITCODE"
'--- EXP3: named args direct (baseline) ---'
& '{S}' -PlanOnly 2>&1 | Select-Object -First 4
"EXP3_EXIT=$LASTEXITCODE"
""".replace("{S}", SCRIPT)

r = subprocess.run(["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", PS],
                   capture_output=True, timeout=60)
print(r.stdout.decode("gbk", "replace"))
err = r.stderr.decode("gbk", "replace")
if err.strip():
    print("[stderr]", err)
