# -*- coding: utf-8 -*-
"""mod.rs 检查数组的括号平衡审计：定位失衡行。"""
import io

lines = io.open("kernel/varix/src/h3star/mod.rs", encoding="utf-8").read().splitlines()
depth = 0
BS = chr(92)
for i, l in enumerate(lines[25:135], start=26):
    out = []
    ins = False
    j = 0
    while j < len(l):
        c = l[j]
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
        if c == "/" and j + 1 < len(l) and l[j + 1] == "/":
            break
        out.append(c)
        j += 1
    code = "".join(out)
    depth += code.count("(") - code.count(")")
    if '("F' in code and depth != 1:
        print("line {}: depth after = {} | tail: {}".format(i, depth, code[-70:]))
print("final depth at line 134:", depth)
