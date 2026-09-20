#!/usr/bin/env python3
"""PS5.1 语法预检（戒律：提权/执行前必须先本地语法 0 错）。

用 PSParser 把脚本整份解析一遍。任何解析错误都会被原样报出来——
包括 BOM 缺失导致的中文串被 GBK 误读这类"语法雪崩"。
"""
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent


def check(script: Path) -> bool:
    if not script.is_file():
        print(f"SKIP: {script} 不存在")
        return False
    # 先看编码：UTF-8 带 BOM 是 .ps1 里含中文的硬要求（否则 PS5.1 按 GBK 读）
    raw = script.read_bytes()
    has_bom = raw.startswith(b"\xef\xbb\xbf")
    try:
        raw.decode("utf-8")
        enc_ok = True
    except UnicodeDecodeError:
        enc_ok = False
    print(f"--- {script.name} ---")
    print(f"  UTF-8 BOM : {'yes' if has_bom else 'NO'}")
    print(f"  合法 UTF-8: {'yes' if enc_ok else 'NO'}")

    ps = (
        "$ErrorActionPreference='Stop';"
        f"$p='{script}';"
        "$t=[IO.File]::ReadAllText($p);"
        "$errs=$null;"
        "[void][System.Management.Automation.PSParser]::Tokenize($t,[ref]$errs);"
        "if($errs -and $errs.Count -gt 0){"
        "  $errs | ForEach-Object { Write-Output ('ERR line ' + $_.Token.StartLine + ': ' + $_.Message) };"
        "  exit 1"
        "} else { Write-Output 'SYNTAX-OK'; exit 0 }"
    )
    r = subprocess.run(
        ["powershell", "-NoProfile", "-NonInteractive", "-Command", ps],
        capture_output=True,
        text=True,
        errors="replace",
    )
    out = (r.stdout or "").strip()
    err = (r.stderr or "").strip()
    print(f"  {out}")
    if err:
        print(f"  stderr: {err}")
    return r.returncode == 0


def main() -> int:
    targets = [Path(a) for a in sys.argv[1:]]
    if not targets:
        targets = [
            ROOT / "_attic" / "fix-dualboot-usb.ps1",
            ROOT / "_attic" / "esp-deploy-switch.ps1",
            ROOT / "_attic" / "esp-dump-ro.ps1",
        ]
    ok = True
    for t in targets:
        if not check(t):
            ok = False
    print()
    print("RESULT:", "ALL-SYNTAX-OK" if ok else "HAS-ERRORS")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
