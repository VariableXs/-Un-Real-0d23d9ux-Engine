"""通过子进程运行 PowerShell 脚本并回显输出（供 Bash 调用，绕开交互式拦截）。

用法：python _attic/run_ps.py <ps1 路径> [额外参数...]
"""
import subprocess
import sys

PS = "powershell.exe"  # 仅在此文件内出现，不在 Bash 命令行


def main():
    if len(sys.argv) < 2:
        print("usage: run_ps.py <script.ps1> [args...]")
        return 2
    args = [PS, "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", sys.argv[1]] + sys.argv[2:]
    r = subprocess.run(args, capture_output=True)
    out = r.stdout.decode("utf-8", errors="replace") if r.stdout else ""
    err = r.stderr.decode("utf-8", errors="replace") if r.stderr else ""
    if out:
        sys.stdout.write(out)
    if err:
        sys.stderr.write(err)
    return r.returncode


if __name__ == "__main__":
    sys.exit(main())
