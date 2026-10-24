#!/usr/bin/env python3
"""死判据重扫（2026-10-22 晚）：找出「定义后全仓零调用」的 run_*_checks。
排除「定义处调用」误判：统计定义以外是否还有出现。输出到 stdout + 文件。
"""
import os, re, sys
from collections import defaultdict

SRC = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel/varix/src"

# 1. 收集所有定义
defs = {}  # fn_name -> relpath
files = []  # (relpath, fullpath)
for root, dirs, fns in os.walk(SRC):
    for fn in fns:
        if fn.endswith(".rs"):
            p = os.path.join(root, fn)
            rel = os.path.relpath(p, SRC).replace("\\", "/")
            files.append((rel, p))

# 2. 全文 token 计数（把每个文件读一次，建 name->count，同时记 name 在哪个文件）
pat_def = re.compile(r"fn (run_\w+_checks)\s*\(")
counts = defaultdict(int)
for rel, p in files:
    try:
        txt = open(p, encoding="utf-8", errors="ignore").read()
    except OSError:
        continue
    for m in pat_def.finditer(txt):
        if m.group(1) not in defs:
            defs[m.group(1)] = rel
    # 出现次数（含定义处）
    for name in set(re.findall(r"run_\w+_checks", txt)):
        counts[name] += txt.count(name)

dead = []
for name, rel in sorted(defs.items()):
    if counts.get(name, 0) <= 1:  # 只出现在定义处
        dead.append((name, rel))

print(f"[scan] 定义总数 {len(defs)}，死判据 {len(dead)} 条")
out = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel/死判据清单-2026-10-22-晚.txt"
with open(out, "w", encoding="utf-8") as f:
    f.write(f"内核死判据清单（重扫 2026-10-22 晚）· 共 {len(dead)} 条\n")
    f.write("生成方式：fn run_*_checks 定义后全仓出现次数 <= 1（仅定义处）\n\n")
    for name, rel in dead:
        f.write(f"run  {name}  ->  ./{rel}\n" if False else f"{name}  ->  ./{rel}\n")
print(f"[out] {out}")
by_dom = defaultdict(int)
for _, rel in dead:
    d = os.path.dirname(rel) or "(top)"
    by_dom[d] += 1
for d, n in sorted(by_dom.items(), key=lambda x: -x[1])[:20]:
    print(f"  {d}: {n}")
