# -*- coding: utf-8 -*-
"""AI-2 · Base-Mother Mock 驱动 v2：清场→Plan→Build→重复Build(断言拒绝)→Verify。"""
import os
import shutil
import stat
import subprocess
import sys

BM = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\portable\engine\Base-Mother.ps1"
OUT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\ai2-basemother-mock"

# Mock 母本带只读属性——rmtree 在 Windows 上清不掉只读文件，必须先摘属性。
base_mock = os.path.join(OUT, "Base.vhdx")
if os.path.exists(base_mock):
    os.chmod(base_mock, stat.S_IWRITE)
    os.remove(base_mock)
shutil.rmtree(OUT, ignore_errors=True)

PS_RUN = (
    "$ErrorActionPreference='Stop'\n"
    "& '" + BM + "' -Action Plan   -Backend Mock -OutDir '" + OUT + "'\n"
    "Write-Host 'MARK-PLAN-OK'\n"
    "& '" + BM + "' -Action Build  -Backend Mock -OutDir '" + OUT + "' -Index 6\n"
    "Write-Host 'MARK-BUILD1-OK'\n"
    "$hit = 'NO'\n"
    "try { & '" + BM + "' -Action Build -Backend Mock -OutDir '" + OUT + "' -Index 6 }\n"
    "catch { $hit = 'CAUGHT' }\n"
    "Write-Host ('MARK-GUARD=' + $hit)\n"
    "if ($hit -ne 'CAUGHT') { Write-Host 'MARK-GUARD-FAIL'; exit 9 }\n"
    "& '" + BM + "' -Action Verify -Backend Mock -OutDir '" + OUT + "'\n"
    "Write-Host 'MARK-VERIFY-OK'\n"
)
out = subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", PS_RUN],
                     capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=180)
print("=== STDOUT ===")
sys.stdout.write(out.stdout)
print("=== STDERR(tail) ===")
sys.stderr.write(out.stderr[-600:])
print("=== RET =", out.returncode, "===")
marks = [m for m in ["MARK-PLAN-OK", "MARK-BUILD1-OK", "MARK-GUARD=CAUGHT", "MARK-VERIFY-OK"] if m in out.stdout]
print("=== MARKS:", marks, "===")
sys.exit(0 if len(marks) == 4 else 1)
