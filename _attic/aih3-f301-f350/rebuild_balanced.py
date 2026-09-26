# -*- coding: utf-8 -*-
"""重建 F336/F337/F344 行并程序化验证括号平衡。"""
import io

p = "kernel/varix/src/h3star/mod.rs"
BS = chr(92)


def strip(line):
    out = []
    ins = False
    j = 0
    while j < len(line):
        c = line[j]
        if ins:
            if c == BS:
                j += 2
                continue
            if c == '"':
                ins = False
            j += 1
            continue
        if c == '"':
            ins = True
            j += 1
            continue
        out.append(c)
        j += 1
    return "".join(out)


def tree(items):
    if len(items) == 1:
        return items[0]
    mid = len(items) // 2
    return "merge_sets({}, {})".format(tree(items[:mid]), tree(items[mid:]))


def balanced(expr):
    code = strip(expr)
    return code.count("(") == code.count(")")


cp_items = [
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
sg_items = [
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

cp_expr = tree(cp_items)
sg_expr = tree(sg_items)
assert balanced('("F336", {}),'.format(cp_expr)), "cp expr unbalanced"
assert balanced('("F344", {}),'.format(sg_expr)), "sg expr unbalanced"

lines = io.open(p, encoding="utf-8").read().splitlines(keepends=True)
for i, l in enumerate(lines):
    if '"F336"' in l and "copypath" in l:
        lines[i] = '        ("F336", {}),\n'.format(cp_expr)
    if '"F337"' in l and "copypath" in l:
        lines[i] = '        ("F337", {}),\n'.format(cp_expr)
    if '"F344"' in l and "sysgov" in l:
        lines[i] = '        ("F344", {}),\n'.format(sg_expr)
io.open(p, "w", encoding="utf-8", newline="\n").write("".join(lines))
print("rebuilt with balance verification")
