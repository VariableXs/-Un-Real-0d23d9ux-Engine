#!/usr/bin/env python3
"""阶段3：svstar2 59 条死判据接线（切片数组 + EOF 补 pub mod + 即时复核）。"""
import os, re

ROOT = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel"
SRC = os.path.join(ROOT, "varix", "src")
P = os.path.join(SRC, "svstar2/mod.rs")

dead = []
for ln in open(os.path.join(ROOT, "死判据清单-2026-10-22.txt"), encoding="utf-8").read().splitlines()[3:]:
    m = re.match(r"(run_\w+)\s+->\s+\.(.+)", ln)
    if m:
        rel = m.group(2).replace("\\", "/").lstrip("/")
        if rel.startswith("svstar2/") and not rel.endswith("mod.rs"):
            dead.append((m.group(1), rel))
assert len(dead) == 59, f"svstar2 死判据 {len(dead)} != 59"

s = open(P, encoding="utf-8", newline="").read()

# 1) 切片数组追加条目
m = re.search(r"let blocks: &\[\(&'static str, fn\(\) -> CheckSet\)\] = &\[", s)
assert m, "找不到 svstar2 切片数组起点"
close = re.search(r"\n\s*\];", s[m.end():])
assert close, "找不到切片收口 ];"
entries = []
for fn, rel in dead:
    head = open(os.path.join(SRC, rel), encoding="utf-8", errors="ignore").read(4000)
    t = re.search(r"\b((?:UNX|VE|CGPU)-F\w+)", head)
    assert t, f"{rel} 无横幅标签"
    entries.append(f'        ("{t.group(1)}", {rel[8:-3]}::{fn}),')
block = "\n// ---- 死判据接线（2026-10-22：59 条定义后零调用，判据从未参与闭环）----\n" + "\n".join(entries)
pos = m.end() + close.start()
s = s[:pos] + "\n" + block + s[pos:]

# 2) 缺声明补 pub mod（EOF）
missing = [(rel, rel[8:-3]) for fn, rel in dead
           if not re.search(r"mod\s+" + re.escape(rel[8:-3]) + r"\s*;", s)]
if missing:
    app = "\n// ---- 死判据补登记（2026-10-22 接线轮：模块早已实现但从未入链）----\n"
    for rel, name in missing:
        first = open(os.path.join(SRC, rel), encoding="utf-8", errors="ignore").readline().strip()[:180]
        app += f"// {first}\npub mod {name};\n"
    if not s.endswith("\n"):
        s += "\n"
    s += app

open(P, "w", encoding="utf-8", newline="").write(s)

# 3) 即时复核
s2 = open(P, encoding="utf-8", newline="").read()
n_wire = s2.count("死判据接线（2026-10-22：59 条")
n_added = len(re.findall(r"^\s*\(\"(?:UNX|VE|CGPU)-F\w+\", \w+::run_\w+_checks\),$",
                         re.search(r"死判据接线（2026-10-22：59 条.*?\n\s*\];", s2, re.S).group(0), re.M))
print(f"[svstar2] 接线块 {n_wire}，块内条目 {n_added}，补声明 {len(missing)} 个")
assert n_added == 59
print("[ok] svstar2 59 条接线完成并复核")
