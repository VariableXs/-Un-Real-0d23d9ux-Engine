#!/usr/bin/env python3
"""check8 残量收敛：format 宏缺失 / 内属性错位 / String/Vec/vec 重复导入。"""
import re
from pathlib import Path

ROOT = Path(r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel")
LOG = ROOT / "_attic" / "check8_full_1022_1955.log"
ERR_LINE = re.compile(r"^\s*-->\s+(varix[\\/][^:]+?):(\d+):(\d+)\s*$")

def read_lines(p):
    raw = p.read_bytes().decode("utf-8", errors="replace")
    eol = "\r\n" if "\r\n" in raw else "\n"
    return raw.split(eol), eol

def write_lines(p, lines, eol):
    p.write_bytes(eol.join(lines).encode("utf-8"))

def collect():
    out = {"fmt": {}, "innerattr": {}, "dup": {}}
    kind = None
    for raw in LOG.read_bytes().decode("utf-8", errors="replace").splitlines():
        if raw.startswith("error"):
            if "cannot find macro `format`" in raw:
                kind = "fmt"
            elif "inner attribute is not permitted" in raw:
                kind = "innerattr"
            elif "E0252" in raw:
                kind = "dup"
            else:
                kind = None
            continue
        m = ERR_LINE.match(raw)
        if m and kind:
            f = m.group(1).replace("\\", "/")
            out[kind].setdefault(f, set()).add(int(m.group(2)))
    return out

def fix_fmt(p, lnos):
    """补 use alloc::format; 插在首个 use 之前。"""
    lines, eol = read_lines(p)
    if any(l.strip() == "use alloc::format;" for l in lines):
        return 0
    for i, l in enumerate(lines):
        if l.strip().startswith("use ") or l.strip().startswith("extern crate"):
            lines.insert(i, "use alloc::format;")
            write_lines(p, lines, eol)
            return 1
    return 0

def fix_innerattr(p, lnos):
    """错位的 #![allow(..)] 挪到文件首行（去掉重复）。"""
    lines, eol = read_lines(p)
    fixed = 0
    moved = []
    for ln in sorted(lnos, reverse=True):
        i = ln - 1
        if i < len(lines) and lines[i].strip().startswith("#!["):
            moved.append(lines[i].strip())
            del lines[i]
            fixed += 1
    if moved:
        # 去重后插到文件顶（跳过 BOM/空行；放最前即可——内属性必须在一切 item 前）
        for attr in dict.fromkeys(moved):
            lines.insert(0, attr)
        write_lines(p, lines, eol)
    return fixed

def fix_dup(p, lnos):
    lines, eol = read_lines(p)
    fixed = 0
    for ln in sorted(lnos, reverse=True):
        i = ln - 1
        if i < len(lines):
            s = lines[i].strip()
            if s.startswith("use ") and s.endswith(";"):
                del lines[i]
                fixed += 1
    if fixed:
        write_lines(p, lines, eol)
    return fixed

def main():
    kinds = collect()
    for kind, fixer in (("fmt", fix_fmt), ("innerattr", fix_innerattr), ("dup", fix_dup)):
        n_files = n_fix = 0
        for f, lnos in kinds[kind].items():
            p = ROOT / f
            if not p.exists():
                print(f"[MISS] {kind} {f}")
                continue
            r = fixer(p, lnos)
            if r:
                n_files += 1
                n_fix += r
        print(f"[{kind}] files={n_files} fixes={n_fix}")

if __name__ == "__main__":
    main()
