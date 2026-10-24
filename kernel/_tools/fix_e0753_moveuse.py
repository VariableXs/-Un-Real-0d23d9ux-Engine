#!/usr/bin/env python3
"""修复「use 注入到内层属性///! 横幅之前」导致的 E0753（2026-10-22 晚）。

原理：内层属性 `#![...]` 与内层文档 `//!` 必须先于一切 item（含 use）。
fix_g3_trio.py 的 inject() 对「文件头不是 //! 而是 #![...]」的文件把 use
插到了第 0 行，使整个横幅变成 E0753。修复：把错位的 use 行移到横幅块之后。
BOM 容错：部分文件带 \ufeff，读入时剥除（Rust 不要求 BOM）。

另修 vek12_checks.rs 的 String 双重导入（组导入已含单名）。
"""
import re, shutil
from pathlib import Path

ROOT = Path(r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel")
SRC = ROOT / "varix" / "src"
BK = ROOT / "_attic" / "e0753fix-backup-2026-10-22"
BK.mkdir(parents=True, exist_ok=True)

targets = set([
    "cgpu/cgb287_teledocs.rs",
    "svstar2/vec23_transunit.rs",
    "svstar2/vef13_checks.rs",
    "svstar2/ves192_assetroll.rs",
    "svstar2/ves194_s07close.rs",
])
# 补扫：首行（剥 BOM/空行）即 use alloc 且后随 #![ 或 //! 的文件
for p in SRC.rglob("*.rs"):
    try:
        text = p.read_text(encoding="utf-8-sig", errors="ignore")
    except OSError:
        continue
    lines = text.split("\n")
    i = 0
    while i < len(lines) and lines[i].strip() == "":
        i += 1
    if i < len(lines) and lines[i].startswith("use alloc::"):
        rest = [l.strip() for l in lines[i + 1:] if l.strip()]
        if rest and (rest[0].startswith("#![") or rest[0].startswith("//!")):
            targets.add(str(p.relative_to(SRC).as_posix()))

print("targets:", len(targets))
fixed = 0
for rel in sorted(targets):
    p = SRC / rel
    text = p.read_text(encoding="utf-8-sig", errors="replace")
    lines = text.split("\n")
    # 头部跑：#![...] / //! / 空行 / use alloc 混合块；use 全部摘出，
    # 插到块内最后一个非 use 行（横幅/属性）之后。
    head_end = 0
    displaced = []
    banner_last = -1
    i = 0
    while i < len(lines):
        s = lines[i].strip()
        if s == "" or s.startswith("#![") or s.startswith("//!"):
            i += 1
            continue
        if s.startswith("use alloc::") and s.endswith(";") and "::" in s:
            displaced.append(lines[i])
            lines.pop(i)
            continue
        break
    if not displaced:
        print("[skip] 无错位 use:", rel)
        continue
    # 横幅结束位置：跳过开头的空行/#![...]///!（use 已摘除）
    idx = 0
    while idx < len(lines):
        s = lines[idx].strip()
        if s == "" or s.startswith("#![") or s.startswith("//!"):
            idx += 1
            continue
        break
    lines[idx:idx] = displaced
    shutil.copy2(p, BK / rel.replace("/", "_"))
    p.write_text("\n".join(lines), encoding="utf-8", newline="")
    fixed += 1
    print(f"[fix] {rel}: 移动 {len(displaced)} 行 use 至横幅后")
print(f"[done] 修复 {fixed}/{len(targets)}")

# vek12 String 去重
p = SRC / "svstar2" / "vek12_checks.rs"
s = p.read_text(encoding="utf-8-sig", errors="replace")
dup = "use alloc::string::String;\n"
if dup in s and "use alloc::string::{String, ToString};" in s:
    s = s.replace(dup, "", 1)
    shutil.copy2(p, BK / "svstar2_vek12_checks.rs")
    p.write_text(s, encoding="utf-8", newline="")
    print("[fix] vek12_checks.rs: 删除重复 use alloc::string::String;")
else:
    print("[skip] vek12 无重复或组导入缺失")
