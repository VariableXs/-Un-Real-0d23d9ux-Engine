#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""[AI-11][C1] 防重终查：UNX-F8301–F8600（B16–B30 轮 300 条）全 repo 机检
判据：①ID 模式命中文件全部落在本域白名单（deepen C1 册/batches C1 册/goal-c1/handoff/总纲/根台账/域脚本）
     ②kernel+userspace+src 源码树零命中 ③他域 deepen/batches 零命中 ④判据号 300 个唯一
"""
import re
import sys
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8")
REPO = Path(__file__).resolve().parents[3]
ok = True

IDPAT = re.compile(r"UNX-F8[3-6]\d\d")
SKIP_DIRS = {".git", "node_modules", "_attic", "__pycache__", "target", "dist", "build"}
SKIP_FILES = {"_fin_out.txt"}  # 本轮校验临时输出（不提交）

whitelist_sub = [
    "docs/unxreal/deepen/C1-",
    "docs/unxreal/batches/UNX-C1-",
    "docs/unxreal/goal-c1.md",
    "docs/unxreal/handoff.json",
    "docs/Varix/CoRun Varix STAR II",
    "CoRun Varix STAR II",
    "docs/unxreal/deepen/_fix_C1_",
    "docs/unxreal/deepen/_finalize_check_C1.py",
    "docs/unxreal/deepen/_backfill_C1_ledgers.py",
    "docs/unxreal/deepen/_dedup_final_C1.py",
]

hits = {}
scanned = 0
for p in REPO.rglob("*"):
    if not p.is_file():
        continue
    rel = p.relative_to(REPO).as_posix()
    if any(part in SKIP_DIRS for part in p.parts):
        continue
    if p.name in SKIP_FILES:
        continue
    if p.suffix.lower() not in {".md", ".json", ".py", ".rs", ".ts", ".tsx", ".js", ".toml", ".txt"}:
        continue
    scanned += 1
    try:
        txt = p.read_text(encoding="utf-8", errors="ignore")
    except OSError:
        continue
    ids = sorted(set(IDPAT.findall(txt)))
    if ids:
        hits[rel] = ids

illegal, legal = [], []
for rel, ids in sorted(hits.items()):
    if any(rel.startswith(w) or rel == w for w in whitelist_sub):
        legal.append((rel, ids))
    else:
        illegal.append((rel, ids))

print(f"扫描 {scanned} 文件；ID 模式命中 {len(hits)} 文件（合法 {len(legal)} / 非法 {len(illegal)}）")
for rel, ids in legal:
    print(f"  [LEGIT] {rel}（{len(ids)} ID：{ids[0]}..{ids[-1]}）")
for rel, ids in illegal:
    print(f"  [RED] {rel}（{len(ids)} ID：{ids[:8]}…）")
if illegal:
    ok = False

# 内核/用户态源码树专项
khit = [rel for rel in hits if rel.startswith(("kernel/", "userspace/", "src/"))]
print(f"kernel+userspace+src 源码树命中：{len(khit)}" + (" —— [RED]" if khit else " —— [OK] 零引用"))
if khit:
    ok = False

# 他域 deepen/batches 专项（排除条件复用 whitelist_sub：_ 开头的本域脚本同属合法，避免检查器自身误报）
ohter = [rel for rel in hits
         if rel.startswith(("docs/unxreal/deepen/", "docs/unxreal/batches/"))
         and not any(rel.startswith(w) or rel == w for w in whitelist_sub)]
print(f"他域 deepen/batches 命中：{len(ohter)}" + (" —— [RED]" if ohter else " —— [OK] 零撞号"))
if ohter:
    ok = False

# 判据号唯一性（300 个 J1）
crit = set()
dup = 0
for b in range(16, 31):
    for line in (REPO / "docs/unxreal/deepen" / f"C1-B{b:02d}.md").read_text(encoding="utf-8").splitlines():
        m = re.match(r"- 域/批：C1/B\d+｜判据：(UNX-F\d{4}-J\d+)", line)
        if m:
            if m.group(1) in crit:
                dup += 1
            crit.add(m.group(1))
print(f"判据号唯一性：{len(crit)}/300，重复 {dup}" + (" —— [OK]" if len(crit) == 300 and dup == 0 else " —— [RED]"))
if len(crit) != 300 or dup:
    ok = False

print(f"\n{'[DONE] 防重终查全绿：F8301–F8600 全 repo 零撞号，白名单外零命中，判据号 300 唯一。' if ok else '[FAIL] 防重终查存在红灯。'}")
sys.exit(0 if ok else 1)
