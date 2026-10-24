#!/usr/bin/env python3
"""check10 残量收敛：E0753 / 内属性 / 缺宏导入 / S9*.ok() 误用 / 缺 Vec。"""
import re
from pathlib import Path

ROOT = Path(r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel")
LOG = ROOT / "_attic" / "check10_full_1022_2050.log"
ERR_LINE = re.compile(r"^\s*-->\s+(varix[\\/][^:]+?):(\d+):(\d+)\s*$")

def read_lines(p):
    raw = p.read_bytes().decode("utf-8", errors="replace")
    eol = "\r\n" if "\r\n" in raw else "\n"
    return raw.split(eol), eol

def write_lines(p, lines, eol):
    p.write_bytes(eol.join(lines).encode("utf-8"))

def collect():
    out = {k: {} for k in ("e0753", "innerattr", "fmt", "vecmacro", "vectype", "okcall")}
    kind = None
    for raw in LOG.read_bytes().decode("utf-8", errors="replace").splitlines():
        if raw.startswith("error"):
            if "E0753" in raw:
                kind = "e0753"
            elif "inner attribute" in raw:
                kind = "innerattr"
            elif "cannot find macro `format`" in raw:
                kind = "fmt"
            elif "cannot find macro `vec`" in raw:
                kind = "vecmacro"
            elif re.search(r"cannot find (type|value) `Vec`", raw) or "E0433" in raw and "`Vec`" in raw:
                kind = "vectype"
            elif "no method named `ok`" in raw:
                kind = "okcall"
            else:
                kind = None
            continue
        m = ERR_LINE.match(raw)
        if m and kind:
            f = m.group(1).replace("\\", "/")
            out[kind].setdefault(f, set()).add(int(m.group(2)))
    return out

def inject_import(lines, stmt):
    if any(l.strip() == stmt for l in lines):
        return False
    for i, l in enumerate(lines):
        s = l.strip()
        if s.startswith("use ") or s.startswith("extern crate"):
            lines.insert(i, stmt)
            return True
    # 没有 use 区：放在第一个空行前/文件头注释后——插到第一个非注释行前
    for i, l in enumerate(lines):
        if l.strip() and not l.strip().startswith("//") and not l.strip().startswith("#!"):
            lines.insert(i, stmt)
            return True
    return False

def fix_e0753(p, lnos):
    lines, eol = read_lines(p)
    fixed = 0
    for ln in sorted(lnos):
        i = ln - 1
        if i < len(lines) and lines[i].lstrip().startswith("//!"):
            lines[i] = lines[i].replace("//!", "//", 1)
            fixed += 1
    if fixed:
        write_lines(p, lines, eol)
    return fixed

def fix_innerattr(p, lnos):
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
        for attr in dict.fromkeys(moved):
            lines.insert(0, attr)
        write_lines(p, lines, eol)
    return fixed

def fix_import(p, lnos, stmt):
    lines, eol = read_lines(p)
    if inject_import(lines, stmt):
        write_lines(p, lines, eol)
        return 1
    return 0

def fix_okcall(p, lnos):
    lines, eol = read_lines(p)
    fixed = 0
    for ln in sorted(lnos):
        i = ln - 1
        while i >= 0 and i < len(lines):
            if ".ok();" in lines[i]:
                lines[i] = lines[i].replace(".ok();", ";", 1)
                fixed += 1
                break
            # 报错行本身可能只有表达式尾，向上找 .ok(); 的行
            if ln - 1 == i:
                # 先在报错行找
                pass
            break
    if fixed:
        write_lines(p, lines, eol)
    return fixed

def main():
    kinds = collect()
    plan = [
        ("e0753", fix_e0753, None),
        ("innerattr", fix_innerattr, None),
        ("fmt", fix_import, "use alloc::format;"),
        ("vecmacro", fix_import, "use alloc::vec;"),
        ("vectype", fix_import, "use alloc::vec::Vec;"),
        ("okcall", fix_okcall, None),
    ]
    for kind, fixer, arg in plan:
        n_files = n_fix = 0
        for f, lnos in kinds[kind].items():
            p = ROOT / f
            if not p.exists():
                print(f"[MISS] {kind} {f}")
                continue
            r = fixer(p, lnos, arg) if arg else fixer(p, lnos)
            if r:
                n_files += 1
                n_fix += r
        print(f"[{kind}] files={n_files} fixes={n_fix}")

if __name__ == "__main__":
    main()
