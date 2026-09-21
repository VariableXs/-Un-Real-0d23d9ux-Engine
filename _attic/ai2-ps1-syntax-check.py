# -*- coding: utf-8 -*-
"""S3.1 辅助：Engine-Chain.ps1 语法先验（PSParser 0 错才执行）+ Mock 演练入口。"""
import subprocess
import sys

PS1 = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\portable\engine\Engine-Chain.ps1"

PS_CHECK = r"""
$p = '__PS1__'
$errs = $null
[System.Management.Automation.PSParser]::Tokenize((Get-Content -LiteralPath $p -Raw), [ref]$errs) | Out-Null
Write-Output ("PS_ERRORS=" + $errs.Count)
foreach ($e in $errs) { Write-Output ("ERR|" + $e.Token.StartLine + "|" + $e.Message) }
""".replace("__PS1__", PS1)

out = subprocess.run(
    ["powershell", "-NoProfile", "-NonInteractive", "-Command", PS_CHECK],
    capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=120,
)
sys.stdout.write(out.stdout)
if out.returncode != 0:
    sys.stderr.write(out.stderr)
    sys.exit(out.returncode)
