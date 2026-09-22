# -*- coding: utf-8 -*-
"""把 repo 根的 boot-select.json 写到 U 盘 SHARED 卷（真相源）。
提权运行：diskpart 挂盘符 → 覆盖写入 → 回读哈希 → 摘盘符。"""
import hashlib
import os
import subprocess
import sys

REPO_CFG = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\boot-select.json"
HERE = os.path.dirname(os.path.abspath(__file__))
LOG = os.path.join(HERE, "vx-shared-cfg.rpt")
lines = []


def run(cmd, desc):
    p = subprocess.run(cmd, capture_output=True)
    out = (p.stdout + p.stderr).decode("gbk", "replace").strip()
    lines.append(f"[{desc}] rc={p.returncode}\n{out}")
    return p.returncode


def main():
    # 1) diskpart：SHARED 分区挂 T:
    dp = os.path.join(os.environ["TEMP"], "vx-shared-assign.txt")
    with open(dp, "w", encoding="ascii") as f:
        f.write("select disk 1\r\nselect partition 4\r\nassign letter=T\r\n")
    run(["diskpart", "/s", dp], "assign T:")
    # 2) 卷标确认
    run(["cmd", "/c", "vol T:"], "vol check")
    # 3) 写入配置（Python 原生复制，避免 cmd 引号转发损坏）
    try:
        import shutil
        shutil.copyfile(REPO_CFG, "T:\\boot-select.json")
        lines.append("write cfg: ok (shutil)")
    except Exception as e:
        lines.append(f"write cfg FAIL: {e}")
    # 4) 回读哈希对比
    try:
        a = hashlib.sha256(open(REPO_CFG, "rb").read()).hexdigest()
        b = hashlib.sha256(open("T:\\boot-select.json", "rb").read()).hexdigest()
        lines.append(f"hash repo ={a}\nhash shared={b}\nmatch={a == b}")
    except Exception as e:
        lines.append(f"hash-verify-fail: {e}")
    # 5) 摘盘符（保持现场干净）
    dp2 = os.path.join(os.environ["TEMP"], "vx-shared-remove.txt")
    with open(dp2, "w", encoding="ascii") as f:
        f.write("select disk 1\r\nselect partition 4\r\nremove letter=T\r\n")
    run(["diskpart", "/s", dp2], "remove T:")
    lines.append("SHARED-CFG-DONE")


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
        import time
        me = os.path.abspath(__file__)
        rc = ctypes.windll.shell32.ShellExecuteW(
            None, "runas", sys.executable, f'"{me}" --elevated', None, 0)
        print("ShellExecute rc=", rc)
        for _ in range(90):
            time.sleep(2)
            if os.path.exists(LOG):
                print(open(LOG, encoding="utf-8").read())
                break
        else:
            print("timeout waiting report")
