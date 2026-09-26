# -*- coding: utf-8 -*-
"""极简括号审计：打印所有括号增量非零的行（80-135 区间）。"""
import io

lines = io.open("kernel/varix/src/h3star/mod.rs", encoding="utf-8").read().splitlines()
for i in range(79, 135):
    l = lines[i]
    op = l.count("(")
    cl = l.count(")")
    if op != cl:
        print(i + 1, "delta", op - cl, "|", l.strip()[:70])
