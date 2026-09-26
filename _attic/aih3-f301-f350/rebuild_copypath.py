# -*- coding: utf-8 -*-
"""重建 copypath 检查树行（12 项）。"""
import io

p = "kernel/varix/src/h3star/mod.rs"
items = [
    "copypath::run_cmdbg_checks()",
    "copypath::run_cmdbg_deep2_checks()",
    "copypath::run_copypath_deep3_checks()",
    "copypath::run_copypath_deep4_checks()",
    "copypath::run_copypath_deep5_checks()",
    "copypath::run_copypath_deep6_checks()",
    "copypath::run_copypath_deep7_checks()",
    "copypath::run_copypath_deep8_checks()",
    "copypath::run_copypath_deep9_checks()",
    "copypath::run_copypath_deep10_checks()",
    "copypath::run_copypath_deep11_checks()",
    "copypath::run_copypath_deep12_checks()",
]


def tree(items):
    if len(items) == 1:
        return items[0]
    mid = len(items) // 2
    return "merge_sets({}, {})".format(tree(items[:mid]), tree(items[mid:]))


lines = io.open(p, encoding="utf-8").read().splitlines(keepends=True)
for i, l in enumerate(lines):
    if '"F336"' in l and "copypath" in l:
        lines[i] = '        ("F336", {}),\n'.format(tree(items))
    if '"F337"' in l and "copypath" in l:
        lines[i] = '        ("F337", {}),\n'.format(
            tree(items[:-1] + ["copypath::run_copypath_deep12_checks()"])
        )
io.open(p, "w", encoding="utf-8", newline="\n").write("".join(lines))
print("copypath trees rebuilt")
