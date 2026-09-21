# -*- coding: utf-8 -*-
"""S3.1 Mock 演练驱动：Plan/New/Health/Drill x10（Mock 后端，_attic 隔离落点）。"""
import subprocess
import sys

PS1 = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\portable\engine\Engine-Chain.ps1"
OUT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\ai2-s31-mock"

RUN = r"""
$ErrorActionPreference = 'Stop'
& '__PS1__' -Action Plan   -Backend Mock -OutDir '__OUT__'
& '__PS1__' -Action New    -Backend Mock -OutDir '__OUT__'
& '__PS1__' -Action Health -Backend Mock -OutDir '__OUT__'
& '__PS1__' -Action Drill  -Backend Mock -OutDir '__OUT__' -Rounds 10
""".replace("__PS1__", PS1).replace("__OUT__", OUT)

out = subprocess.run(
    ["powershell", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", RUN],
    capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=300,
)
sys.stdout.write(out.stdout)
if out.stderr:
    sys.stderr.write("[stderr] " + out.stderr[:2000])
sys.exit(out.returncode or 0)
