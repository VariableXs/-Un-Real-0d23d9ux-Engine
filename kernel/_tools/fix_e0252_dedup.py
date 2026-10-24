#!/usr/bin/env python3
"""[已废弃 · 禁止运行] fix_e0252_dedup.py

判定缺陷：以「文件级 alloc_set 含 alloc::vec」为删除条件，但独立
`use alloc::vec;` 行本身就会让 alloc_set 命中 ⇒ 任何含独立 vec 行的文件
都会被无条件删行，包括该行是唯一 vec 绑定的文件（2026-10-22 晚实测误删
1279+1469 处，已从备份恢复）。

正确替代：`_tools/fix_use_dedup.py`（全路径覆盖图判定，只有每条绑定路径
都另有语句提供时才删）。本文件保留仅作事故记录。
"""
import sys

print("[BLOCKED] 本脚本有已知误删缺陷，禁止运行；请改用 _tools/fix_use_dedup.py")
sys.exit(2)

import os, re, shutil, sys  # noqa: E402 以下原实现永不执行
from pathlib import Path

TOOLS = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel/_tools"
sys.path.insert(0, TOOLS)
import varix_quality_gate as vg

ROOT = Path(r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel")
SRC = ROOT / "varix" / "src"
BK = ROOT / "_attic" / "g3fix-backup-2026-10-22"

fixed = 0
scanned = 0
for rs in vg.iter_rs_files(SRC):
    try:
        text = rs.read_text(encoding="utf-8", errors="replace")
    except OSError:
        continue
    # 快筛：是否存在独立 use alloc::vec; 行
    standalone = re.search(r"^[ \t]*use alloc::vec;[ \t]*\r?\n", text, re.M)
    if not standalone:
        continue
    scanned += 1
    # 用门禁展开器收集真绑定
    alloc_set = set(vg._collect_alloc_uses(text).split())
    if "alloc::vec" in alloc_set:
        # 独立行 + 括号组绑定 = 重复；删除独立行
        new_text = re.sub(r"^[ \t]*use alloc::vec;[ \t]*\r?\n", "", text, count=1, flags=re.M)
        # 备份当前态，再写
        rel = rs.relative_to(ROOT).as_posix().replace("/", "_")
        shutil.copy2(rs, BK / ("dedup_" + rel))
        rs.write_text(new_text, encoding="utf-8", newline="")
        fixed += 1
        print(f"[dedup] {rs.relative_to(SRC.parent)}")

print(f"[done] 扫描 {scanned} 个含独立 vec 行的文件，删除重复 {fixed} 处")
