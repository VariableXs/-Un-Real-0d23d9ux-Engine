#!/usr/bin/env python3
"""G3 no_std 三件套批量修复（2026-10-22 晚 · v2）。

直接复用 varix_quality_gate.py 的检测原语（同正则、同剥注释、同 cfg(test) 剔除），
对每个缺项文件注入缺失的 use alloc::...;，只注入不改逻辑，幂等，备份齐全。
"""
import os, re, shutil, sys
from pathlib import Path

TOOLS = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel/_tools"
sys.path.insert(0, TOOLS)
import varix_quality_gate as vg  # noqa: E402

ROOT = Path(r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel")
SRC = ROOT / "varix" / "src"
BK = ROOT / "_attic" / "g3fix-backup-2026-10-22"

log = []
def say(m):
    print(m); log.append(m)

# ---------- 1. 全量检测（与门禁 G3 同逻辑）----------
tp_roots = vg.third_party_roots(SRC)
missing = []
for rs in vg.iter_rs_files(SRC):
    if vg.is_third_party(rs, tp_roots):
        continue
    try:
        text = rs.read_text(encoding="utf-8", errors="replace")
    except OSError:
        continue
    lines = text.splitlines()
    in_test = vg.strip_test_scopes(lines)
    stripped = []
    in_block = False
    for ln, is_t in zip(lines, in_test):
        if is_t:
            stripped.append(""); continue
        code_only, in_block = vg._strip_comments(ln, in_block)
        stripped.append(code_only)
    ktext = "\n".join(stripped)
    if not ktext.strip():
        continue
    need_macro = bool(vg.USE_VEC_MACRO.search(ktext))
    need_type = bool(vg.USE_VEC_TYPE.search(ktext))
    need_string = bool(vg.USE_STRING_TYPE.search(ktext) or vg.USE_STRING_MACRO.search(ktext))
    if not (need_macro or need_type or need_string):
        continue
    if re.search(r"^\s*(pub\s+)?use\s+std::", text, re.M):
        continue
    alloc_set = vg._collect_alloc_uses(text).split()
    items = []
    if need_macro and "alloc::vec" not in alloc_set:
        items.append("alloc::vec")
    if need_type and "alloc::vec::Vec" not in alloc_set:
        items.append("alloc::vec::Vec")
    if need_string and not (
        "alloc::string::String" in alloc_set or "alloc::string::ToString" in alloc_set
    ):
        items.append("alloc::string::{String, ToString}")
    if items:
        missing.append((rs, items))

say(f"[scan] 缺项文件 {len(missing)} 个")

# ---------- 2. 二次防线：防重复绑定（E0252）----------
# 2026-10-22 晚修正：改用门禁的递归花括号展开器 _collect_alloc_uses，
# 旧版自写正则对嵌套组（string::{String, ToString} 外层还有组）会截断，
# 漏认 vec/vec::Vec 绑定导致多注入 → E0252（已由 fix_e0252_dedup.py 清理）。
def gate_alloc_set(text: str) -> set[str]:
    return set(vg._collect_alloc_uses(text).split())

def needs(text: str, item: str, alloc_set: set[str]) -> bool:
    """与门禁 G3 同判据：item 是否缺失。"""
    if item == "alloc::vec":
        return "alloc::vec" not in alloc_set
    if item == "alloc::vec::Vec":
        return "alloc::vec::Vec" not in alloc_set
    if item == "alloc::string::{String, ToString}":
        return not ("alloc::string::String" in alloc_set or "alloc::string::ToString" in alloc_set)
    return False

def inject(text: str, items: list[str]) -> str:
    lines = text.split("\n")
    idx = 0
    for i, ln in enumerate(lines):
        s = ln.strip()
        # 内层属性 #![...] 与内层文档 //! 都必须先于一切 item（含 use），
        # 两者都要跳过——漏掉 #![ 会把 use 插到它前面制造 E0753。
        if s.startswith("//!") or s.startswith("#!["):
            idx = i + 1
        elif s == "" and i == 0:
            continue
        elif s == "" and idx == i:
            idx = i + 1
        else:
            break
    block = "\n".join(f"use {u};" for u in items)
    lines.insert(idx, block)
    return "\n".join(lines)

# ---------- 3. 注入 ----------
# 参数保护：--dry-run 只打印计划不写盘；--only <子串>[,<子串>...] 只处理路径含子串的文件。
# （无参数直接跑 = 全量实跑，历史上曾因此误修并行活跃文件，先预览再实跑。）
ARGS = sys.argv[1:]
DRY = "--dry-run" in ARGS or "-n" in ARGS
ONLY = None
for a in ARGS:
    if a.startswith("--only="):
        ONLY = [s for s in a[len("--only="):].split(",") if s]
if ONLY:
    missing = [(rs, it) for rs, it in missing
               if any(s in rs.as_posix() for s in ONLY)]

for rs, it in missing:
    say(f"[plan] {rs.relative_to(SRC.parent)}: {'; '.join(it)}")
if DRY:
    say(f"[dry-run] 计划修复 {len(missing)} 文件（未写盘）")
    sys.exit(0)

BK.mkdir(parents=True, exist_ok=True)
changed = 0
for rs, items in missing:
    text = rs.read_text(encoding="utf-8", errors="replace")
    alloc_set = gate_alloc_set(text)
    todo = [it for it in items if needs(text, it, alloc_set)]
    if not todo:
        continue
    shutil.copy2(rs, BK / rs.relative_to(SRC.parent).as_posix().replace("/", "_"))
    rs.write_text(inject(text, todo), encoding="utf-8", newline="")
    changed += 1
    if changed <= 8 or changed % 50 == 0:
        say(f"[fix] {rs.relative_to(SRC.parent)}: +{'; '.join(todo)}")

say(f"[done] 修复 {changed} / {len(missing)} 文件")
(BK / "g3fix-log.txt").write_text("\n".join(log), encoding="utf-8")
