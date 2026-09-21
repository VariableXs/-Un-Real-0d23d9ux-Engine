# -*- coding: utf-8 -*-
"""AI-4：s207 走查脚本加 QEMU 存活检查（意外死亡=FAIL 该轮而非脚本崩）。"""
import io

P = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\qemu-s207-suite-walkthrough.py"

with io.open(P, "r", encoding="utf-8", newline="") as f:
    src = f.read()

old = '''        opened = False
        for _ in range(5):
            mon.key("ret")
            if wait_count("SHELL: file opened name=", b_open, 90):
                opened = True
                break
            mon.key("down")
        checks.append(("files: open file → preview", opened))'''
new = '''        opened = False
        for _ in range(5):
            if proc.poll() is not None:
                break  # QEMU 意外死亡（外部终止防护），下方存活检查记录
            mon.key("ret")
            if wait_count("SHELL: file opened name=", b_open, 90):
                opened = True
                break
            mon.key("down")
        checks.append(("files: open file → preview", opened))
        if proc.poll() is not None:
            checks.append(("qemu: alive through file preview", False))
            print(f"ROUND {rnd + 1}: QEMU died unexpectedly")
            return sum(1 for _, ok in checks if ok), len(checks)
        checks.append(("qemu: alive through file preview", True))'''

assert old in src, "anchor missing"
src = src.replace(old, new, 1)

with io.open(P, "w", encoding="utf-8", newline="") as f:
    f.write(src)

with io.open(P, "r", encoding="utf-8") as f:
    back = f.read()
print("verify:", "alive through file preview" in back)
