# -*- coding: utf-8 -*-
"""逐 F 行打印括号深度（找余额不为 1 的行）。"""
import io

BS = chr(92)
lines = io.open("kernel/varix/src/h3star/mod.rs", encoding="utf-8").read().splitlines()
depth = 0
for i, l in enumerate(lines[25:140], start=26):
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
        out.append(c)
        j += 1
    code = "".join(out)
    depth += code.count("(") - code.count(")")
    if '("F' in code:
        ok = "OK " if depth == 1 else "BAD"
        print(i, ok, "depth", depth, "|", code[:55])
print("final:", depth)
