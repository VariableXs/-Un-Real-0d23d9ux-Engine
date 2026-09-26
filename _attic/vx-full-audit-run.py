# -*- coding: utf-8 -*-
"""vx-full-audit.ps1 的 BOM 写入 + 语法预验 + 提权执行包装。"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
PS1 = os.path.join(HERE, "vx-full-audit.ps1")
LOG = os.path.join(HERE, "vx-full-audit.rpt")


def prepare():
    import io
    src = io.open(PS1, "r", encoding="utf-8-sig").read()
    io.open(PS1, "w", encoding="utf-8-sig").write(src)  # 确保 UTF-8 BOM
    chk = subprocess_syntax_check(PS1)
    print("syntax check:", chk)
    if "SYNTAX-OK" not in chk:
        sys.exit(1)


def subprocess_syntax_check(path):
    import subprocess
    chk = subprocess.run(
        ["powershell", "-NoProfile", "-Command",
         "$e=$null; [System.Management.Automation.PSParser]::Tokenize("
         "(Get-Content -Raw '" + path + "'), [ref]$e) | Out-Null; "
         "if ($e.Count -eq 0) { 'SYNTAX-OK' } else { $e | ForEach-Object { $_.Message } }"],
        capture_output=True, text=True, timeout=60)
    return chk.stdout.strip() or chk.stderr.strip()[:400]


def run_elevated():
    import ctypes
    import time
    me = os.path.abspath(__file__)
    rc = ctypes.windll.shell32.ShellExecuteW(
        None, "runas", sys.executable, f'"{me}" --elevated', None, 0)
    print("ShellExecute rc=", rc)
    for _ in range(180):
        time.sleep(2)
        if os.path.exists(LOG):
            print(open(LOG, encoding="utf-8").read()[-8000:])
            break
    else:
        print("timeout waiting report")


def elevated_main():
    r = subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass",
                        "-File", PS1], capture_output=True, timeout=600)
    txt = r.stdout.decode("gbk", "replace")
    err = r.stderr.decode("gbk", "replace")
    with open(LOG, "w", encoding="utf-8") as f:
        f.write(txt)
        if err.strip():
            f.write("\n=== STDERR ===\n" + err[:2000])


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--elevated":
        elevated_main()
    else:
        prepare()
        run_elevated()
