# -*- coding: utf-8 -*-
"""离线向 U 盘 Win11 的 Default 用户模板 NTUSER.DAT 写入 Variable 自启动 Run 键。
提权运行（reg load 需要管理员）。只写 Default 模板，不碰任何在线系统。"""
import os
import subprocess
import sys
import time

HIVE = r"X:\Users\Default\NTUSER.DAT"
MOUNT = "HKLM\VXDEFAULT"
RUNKEY = r"HKLM\VXDEFAULT\Software\Microsoft\Windows\CurrentVersion\Run"
CMD = r'"X:\Variable\Variable.exe"'
LOG = os.path.join(os.path.dirname(os.path.abspath(__file__)), "vx-usbwin-autostart.rpt")
lines = []


def run(cmd):
    p = subprocess.run(cmd, capture_output=True)
    out = (p.stdout + p.stderr).decode("gbk", "replace").strip()
    lines.append(f"$ {' '.join(cmd)}\n  rc={p.returncode}\n{out}")
    return p.returncode, out


def main():
    # 若上次残留未卸载，先清理
    run(["reg", "unload", MOUNT])
    rc, _ = run(["reg", "load", MOUNT, HIVE])
    if rc != 0:
        lines.append("LOAD-FAIL")
        return
    run(["reg", "add", RUNKEY, "/v", "VariableDesktop", "/t", "REG_SZ", "/d", CMD, "/f"])
    run(["reg", "query", RUNKEY, "/v", "VariableDesktop"])
    run(["reg", "unload", MOUNT])
    lines.append("USBWIN-AUTOSTART-DONE")


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--elevated":
        try:
            main()
        except Exception:
            import traceback
            lines.append("EXCEPTION:\n" + traceback.format_exc())
        with open(LOG, "w", encoding="utf-8") as f:
            f.write("\n".join(lines) + "\n")
    else:
        import ctypes
        me = os.path.abspath(__file__)
        rc = ctypes.windll.shell32.ShellExecuteW(
            None, "runas", sys.executable, f'"{me}" --elevated', None, 0)
        print("ShellExecute rc=", rc)
        for _ in range(60):
            time.sleep(2)
            if os.path.exists(LOG):
                print(open(LOG, encoding="utf-8").read())
                break
        else:
            print("timeout waiting report")
