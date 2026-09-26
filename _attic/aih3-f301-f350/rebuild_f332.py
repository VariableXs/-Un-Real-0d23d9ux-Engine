# -*- coding: utf-8 -*-
"""重建 F332 行：10 个检查项的平衡 merge_sets 树（程序生成防手数错）。"""
import io

p = "kernel/varix/src/h3star/mod.rs"
items = [
    "animdegrade::run_fpsadapt_checks()",
    "animdegrade::run_animdegrade_deep3_checks()",
    "animdegrade::run_animdegrade_deep4_checks()",
    "animdegrade::run_animdegrade_deep5_checks()",
    "animdegrade::run_animdegrade_deep6_checks()",
    "animdegrade::run_animdegrade_deep7_checks()",
    "animdegrade::run_animdegrade_deep8_checks()",
    "animdegrade::run_animdegrade_deep9_checks()",
    "animdegrade::run_animdegrade_deep10_checks()",
    "animdegrade::run_animdegrade_deep11_checks()",
]


def tree(items):
    if len(items) == 1:
        return items[0]
    mid = len(items) // 2
    left = tree(items[:mid])
    right = tree(items[mid:])
    return "merge_sets({}, {})".format(left, right)


line = '        ("F332", {}),\n'.format(tree(items))

lines = io.open(p, encoding="utf-8").read().splitlines(keepends=True)
for i, l in enumerate(lines):
    if '"F332"' in l and "animdegrade" in l:
        lines[i] = line
io.open(p, "w", encoding="utf-8", newline="\n").write("".join(lines))
print("F332 line rebuilt:", len(line), "chars")
