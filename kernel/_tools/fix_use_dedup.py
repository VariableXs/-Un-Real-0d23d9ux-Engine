#!/usr/bin/env python3
"""模组级 use 冗余去重（2026-10-22 晚 · 治理 E0252 爆发）。

背景：多个批量修复器并行作业（本会话 fix_g3_trio / 他会话 fix_alloc_imports），
对 `use crate::alloc::...` 形态的既有绑定各有盲区，向 300+ 文件注入了重复绑定。

规则（保守，只删真正冗余的整条语句）：
  - 只处理**列 0**（模组级）的 use 语句；函数内缩进 use 不动；
  - 用门禁的 _expand_use_path 展开每条语句的绑定名集合；
  - 一条语句被删，当且仅当它绑定的**每一个名字**都被其它模组级语句绑定
    （先全量收集，再判定，再删除）；
  - 删除前备份到 _attic/use-dedup-backup-2026-10-22/。
"""
import re, shutil
from pathlib import Path
import sys

TOOLS = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS))
import varix_quality_gate as vg

ROOT = Path(r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel")
SRC = ROOT / "varix" / "src"
BK = ROOT / "_attic" / "use-dedup-backup-2026-10-22"
BK.mkdir(parents=True, exist_ok=True)

USE_LINE = re.compile(r"^((?:pub(?:\(crate\))? )?use [^;]+;)(.*)$", re.M)
NAMES_OF_INTEREST = {"vec", "Vec", "String", "ToString", "format"}

def bound_names(clause: str) -> set[str]:
    """展开一条 use 正文（不含 use / 分号），返回绑定名尾集合。"""
    body = re.sub(r"\s+", "", clause)
    if body.startswith("crate::"):
        body = body[len("crate::"):]
    paths = vg._expand_use_path(body)
    names = set()
    for p in paths:
        # 剥掉 alias（as X）
        tail = p.rsplit("::", 1)[-1]
        names.add(tail)
    return names

def expand_paths(clause: str) -> list[str]:
    body = re.sub(r"\s+", "", clause)
    if body.startswith("crate::"):
        body = body[len("crate::"):]
    return vg._expand_use_path(body)

changed = 0
scanned = 0
for rs in SRC.rglob("*.rs"):
    try:
        text = rs.read_text(encoding="utf-8-sig", errors="replace")
    except OSError:
        continue
    lines = text.split("\n")
    # 收集列 0 的 use 语句（可能跨行）：找以 use 开头且列 0 的行，读到 ; 为止
    stmts = []  # (start_line, end_line, names, full_text)
    i = 0
    while i < len(lines):
        ln = lines[i]
        if ln.startswith("use ") or ln.startswith("pub use ") or ln.startswith("pub(crate) use "):
            j = i
            buf = []
            while j < len(lines):
                buf.append(lines[j])
                if ";" in lines[j]:
                    break
                j += 1
            full = "\n".join(buf)
            clause = full.strip()
            clause = re.sub(r"^(?:pub(?:\(crate\))? )?use\s+", "", clause).rstrip(";").strip()
            names = bound_names(clause)
            stmts.append((i, j, names, full))
            i = j + 1
            continue
        i += 1
    if len(stmts) < 2:
        continue
    scanned += 1
    # 全量名字覆盖图
    all_paths: dict[str, list[int]] = {}
    for k, (s, e, names, _f) in enumerate(stmts):
        for p in expand_paths(re.sub(r"^(?:pub(?:\(crate\))? )?use\s+", "", stmts[k][3].strip()).rstrip(";").strip()):
            if p.startswith("crate::"):
                p = p[len("crate::"):]
            all_paths.setdefault(p, []).append(k)
    # 判定冗余语句：其展开出的每条完整路径都有其它语句也提供
    redundant = []
    for k, (s, e, names, _f) in enumerate(stmts):
        clause = re.sub(r"^(?:pub(?:\(crate\))? )?use\s+", "", stmts[k][3].strip()).rstrip(";").strip()
        paths = [p[len("crate::"):] if p.startswith("crate::") else p
                 for p in expand_paths(clause)]
        if not paths:
            continue
        if not (names & NAMES_OF_INTEREST):
            continue  # 只治理 alloc 三件套相关名，别的不碰
        if all(len(all_paths.get(p, [])) > 1 for p in paths):
            # 防倒退伤：若语句前方（隔空行）紧贴 #[...] 属性，删除会留下
            # 游离属性门控下一条 use（vel09_checks 事故形态，2026-10-22 实证）。
            a = s - 1
            while a >= 0 and lines[a].strip() == "":
                a -= 1
            if a >= 0 and lines[a].lstrip().startswith("#["):
                continue
            redundant.append((k, s, e))
    if not redundant:
        continue
    # 删除（从后往前）
    shutil.copy2(rs, BK / rs.relative_to(SRC.parent).as_posix().replace("/", "_"))
    for k, s, e in sorted(redundant, reverse=True):
        del lines[s:e + 1]
    rs.write_text("\n".join(lines), encoding="utf-8", newline="")
    changed += 1
    if changed <= 10 or changed % 50 == 0:
        print(f"[dedup] {rs.relative_to(SRC.parent)}: 删 {len(redundant)} 条冗余 use")

print(f"[done] 扫描 {scanned} 个多语句文件，修复 {changed} 个")
