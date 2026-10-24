#!/usr/bin/env python3
"""恢复 unxstar/mod.rs：把 194 条错插条目从 run_unx22_batch 撤出，插回 unxstar_blocks。"""
import os, re

P = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel/varix/src/unxstar/mod.rs"
s = open(P, encoding="utf-8", newline="").read()

# 1) 从 run_unx22_batch 撤出错插条目
m = re.search(r"let entries: \[\(&'static str, bool, &'static str\); 11\] = \[", s)
assert m, "找不到 run_unx22_batch entries 起点"
start = m.end()
close = re.search(r"\n    \];", s[start:])
assert close, "找不到 run_unx22_batch 收口"
region = s[start:start + close.start()]
bad_pat = re.compile(r"\n\s*\(\"UNX-F\w+\", \w+::run_\w+_checks as fn\(\) -> CheckSet\),")
removed = bad_pat.findall(region)
assert len(removed) == 194, f"撤出条目 {len(removed)} != 194"
region_fixed = bad_pat.sub("", region)
s = s[:start] + region_fixed + s[start + close.start():]
print(f"[fix] 从 run_unx22_batch 撤出 {len(removed)} 条")

# 2) 还原两处注释里的容量字面量（263 -> 69，仅注释行）
s = re.sub(r"(//.*?); 263\]", r"\1; 69]", s)

# 3) 插入 unxstar_blocks：裸 `    ]` 收口（函数直接返回数组字面量）
m2 = re.search(r"pub fn unxstar_blocks\(\)", s)
assert m2, "找不到 unxstar_blocks"
tail = s[m2.end():]
close2 = re.search(r"\n    \]\n\}", tail)
assert close2, "找不到 unxstar_blocks 裸 ] 收口"

# 重新收集 194 条（与第一遍同源：死判据清单 + 横幅标签）
ROOT = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel"
SRC = os.path.join(ROOT, "varix", "src")
dead = []
for ln in open(os.path.join(ROOT, "死判据清单-2026-10-22.txt"), encoding="utf-8").read().splitlines()[3:]:
    mm = re.match(r"(run_\w+)\s+->\s+\.(.+)", ln)
    if mm:
        rel = mm.group(2).replace("\\", "/").lstrip("/")
        if rel.startswith("unxstar/") and not rel.endswith("mod.rs"):
            dead.append((mm.group(1), rel))
assert len(dead) == 194, f"unxstar 死判据 {len(dead)} != 194"
entries = []
for fn, rel in dead:
    head = open(os.path.join(SRC, rel), encoding="utf-8", errors="ignore").read(4000)
    t = re.search(r"\b((?:UNX|VE|CGPU)-F\w+)", head)
    assert t, f"{rel} 无标签"
    entries.append(f'        ("{t.group(1)}", {rel[8:-3]}::{fn} as fn() -> CheckSet),')
block = "\n" + "\n".join(entries)
pos = m2.end() + close2.start()
s = s[:pos] + block + s[pos:]
open(P, "w", encoding="utf-8", newline="").write(s)

# 4) 复核
s2 = open(P, encoding="utf-8", newline="").read()
m3 = re.search(r"pub fn unxstar_blocks\(\) -> \[\(&'static str, fn\(\) -> CheckSet\); (\d+)\] \[(.*?)\n    \]\n\}", s2, re.S)
n = len(re.findall(r"^\s*\(\"", m3.group(2), re.M))
assert n == int(m3.group(1)) == 263, f"unxstar_blocks {n} != {m3.group(1)} != 263"
m4 = re.search(r"let entries: \[\(&'static str, bool, &'static str\); 11\] = \[(.*?)\n    \];", s2, re.S)
n4 = len(re.findall(r"^\s*\(\"", m4.group(1), re.M))
assert n4 == 11, f"run_unx22_batch {n4} != 11"
print(f"[ok] unxstar_blocks 复核 {n} == 263；run_unx22_batch 复核 {n4} == 11")
