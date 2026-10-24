#!/usr/bin/env python3
"""E0252 use 去重 v2 —— 修复「逐行 use + 注入聚合式 use 同名冲突」。

背景：2026-10-23 00:42 并行批量注入三件套时未识别文件内既有 use（含
crate::alloc:: 等价形态），造成全仓 40+ 文件 E0252。本脚本按名字（末段）
记账，从上往下保留先出现者，整删或改写后出现的重复项。glob（use x::*）
不与显式导入冲突，跳过。只处理 --files 点名的文件，绝不全仓扫。

用法：
  python _tools/fix_e0252_dedup2.py --files a.rs b.rs          # 实修
  python _tools/fix_e0252_dedup2.py --dry-run --files a.rs ... # 预览
"""
import re
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BK = ROOT / "_attic" / "e0252-dedup2-backup"

ARGS = sys.argv[1:]
DRY = "--dry-run" in ARGS or "-n" in ARGS
i = ARGS.index("--files") if "--files" in ARGS else -1
FILES = [Path(f) for f in ARGS[i + 1:]] if i >= 0 else []
if not FILES:
    print("usage: fix_e0252_dedup2.py [--dry-run] --files <rs...>")
    sys.exit(2)


def split_top(s: str):
    """按深度 0 的逗号切分。"""
    out, buf, d = [], "", 0
    for c in s:
        if c == "{":
            d += 1
        elif c == "}":
            d -= 1
        if c == "," and d == 0:
            out.append(buf)
            buf = ""
        else:
            buf += c
    if buf.strip():
        out.append(buf)
    return [x.strip() for x in out if x.strip()]


def expand(body: str):
    """use 体 → 完整路径列表。递归展开嵌套花括号。

    例：'alloc::{format, string::{String, ToString}, vec::Vec}'
      → ['alloc::format', 'alloc::string::String',
         'alloc::string::ToString', 'alloc::vec::Vec']
    """
    names = []
    for part in split_top(body):
        if "{" in part:
            head, rest = part.split("{", 1)
            if not rest.endswith("}"):
                raise ValueError(f"花括号不配对: {part!r}")
            rest = rest[:-1]  # 剥掉与该 { 配对的 }
            names.extend(expand_with_prefix(rest, prefix=head))
        else:
            names.append(part)
    return names


def expand_with_prefix(body: str, prefix: str):
    names = []
    for part in split_top(body):
        if "{" in part:
            head, rest = part.split("{", 1)
            rest = rest[:-1]
            names.extend(expand_with_prefix(rest, prefix=prefix + head))
        else:
            names.append(prefix + part)
    return names


def last_seg(path: str):
    p = path.strip()
    if p.endswith("::*"):
        return None  # glob
    return p.split("::")[-1]


def fix_text(text: str):
    """返回 (新文本, 变更描述列表)。逐顶层 use 语句处理，保留先出现者。"""
    changes = []
    stmts = []
    for m in re.finditer(r"(?m)^[ \t]*use\s+", text):
        start = m.start()
        end = text.find(";", m.end())
        if end == -1:
            continue
        end += 1
        stmts.append((start, end, text[m.end():end - 1]))

    seen = set()
    edits = []
    for start, end, body in stmts:
        if "::*" in body:
            continue  # glob 不参与冲突
        paths = expand(body)
        names = [last_seg(p) for p in paths]
        dup = [n for n in names if n in seen]
        if not dup:
            seen.update(n for n in names if n)
            continue
        keep = [p for p, n in zip(paths, names) if n not in seen]
        seen.update(n for n in names if n)
        if not keep:
            edits.append((start, end, ""))
            changes.append(f"删整句 use {body.strip()[:50]}（全重复: {'、'.join(dup)}）")
        else:
            # 全部以语句根（第一个 :: 前的段）重建为逐条单名 use
            root = body.strip().split("::")[0].strip()
            repl = "\n".join(f"use {root}::{p.split('::', 1)[1] if '::' in p else p};"
                             for p in keep)
            edits.append((start, end, repl))
            changes.append(f"改写 use {body.strip()[:40]}… → 保留 {len(keep)} 项（去重 {'、'.join(dup)}）")

    out = text
    for start, end, repl in sorted(edits, key=lambda e: -e[0]):
        out = out[:start] + repl + out[end:]
    out = re.sub(r"\n{3,}", "\n\n", out)
    return out, changes


for f in FILES:
    f = f if f.is_absolute() else (ROOT / f)
    text = f.read_text(encoding="utf-8", errors="replace")
    new, changes = fix_text(text)
    rel = f.relative_to(ROOT).as_posix()
    if not changes:
        print(f"[skip] {rel}: 无重复")
        continue
    print(f"[{'dry' if DRY else 'fix'}] {rel}: {'; '.join(changes)}")
    if not DRY:
        BK.mkdir(parents=True, exist_ok=True)
        bkp = BK / rel.replace("/", "_")
        if not bkp.exists():
            shutil.copy2(f, bkp)
        f.write_text(new, encoding="utf-8", newline="")
print(f"[done] {len(FILES)} 文件处理完成（dry-run={DRY}）")
