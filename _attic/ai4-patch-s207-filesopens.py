# -*- coding: utf-8 -*-
"""AI-4：s207 脚本 files:opens 判定简化（R2 时序教训：菜单中间态判定绕且脆，
改为复合动作后只断言最终 files opened 标记增量）。"""
import io

P = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\qemu-s207-suite-walkthrough.py"

with io.open(P, "r", encoding="utf-8", newline="") as f:
    src = f.read()

old = '''        # ④ 文件管理器：菜单第 1 项。
        mon.key("ret")
        ok = wait_count("SHELL: startmenu opened", b_menu + 1 if count_marker("SHELL: startmenu opened") == b_menu else b_menu)
        mon.key("ret")
        b_files = count_marker("SHELL: files opened")
        mon.key("ret")
        checks.append(("files: opens", wait_count("SHELL: files opened", b_files)))'''
new = '''        # ④ 文件管理器：esc 确认回桌面后 ret 开菜单 → ret 进第 1 项
        # （R2 教训：菜单中间态判定绕且脆——只断言最终标记增量）。
        mon.key("esc")
        time.sleep(1.5)
        mon.key("ret")
        time.sleep(1.5)
        b_files = count_marker("SHELL: files opened")
        mon.key("ret")
        checks.append(("files: opens", wait_count("SHELL: files opened", b_files)))'''

assert old in src, "anchor missing"
src = src.replace(old, new, 1)

with io.open(P, "w", encoding="utf-8", newline="") as f:
    f.write(src)

with io.open(P, "r", encoding="utf-8") as f:
    back = f.read()
print("verify:", "只断言最终标记增量" in back)
