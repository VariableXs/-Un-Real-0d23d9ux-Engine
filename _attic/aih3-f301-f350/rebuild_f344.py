# -*- coding: utf-8 -*-
"""重建 F344 行：10 个检查项的平衡 merge_sets 树。"""
import io

p = "kernel/varix/src/h3star/mod.rs"
items = [
    "sysgov::run_appuninst_checks()",
    "sysgov::run_sysgov_deep3_checks()",
    "sysgov::run_sysgov_deep4_checks()",
    "sysgov::run_sysgov_deep5_checks()",
    "sysgov::run_sysgov_deep6_checks()",
    "sysgov::run_sysgov_deep7_checks()",
    "sysgov::run_sysgov_deep8_checks()",
    "sysgov::run_sysgov_deep9_checks()",
    "sysgov::run_sysgov_deep10_checks()",
    "sysgov::run_sysgov_deep11_checks()",
    "sysgov::run_sysgov_deep12_checks()",
]


def tree(items):
    if len(items) == 1:
        return items[0]
    mid = len(items) // 2
    return "merge_sets({}, {})".format(tree(items[:mid]), tree(items[mid:]))


line = '        ("F344", {}),\n'.format(tree(items))

lines = io.open(p, encoding="utf-8").read().splitlines(keepends=True)
found = False
for i, l in enumerate(lines):
    if '"F344"' in l and "sysgov" in l:
        lines[i] = line
        found = True
assert found, "F344 line not found"
io.open(p, "w", encoding="utf-8", newline="\n").write("".join(lines))
print("F344 line rebuilt")
