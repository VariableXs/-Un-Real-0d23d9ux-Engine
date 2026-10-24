#!/usr/bin/env python3
"""check9 残量收敛：item 间 //! → //（E0753）/ 内属性挪顶 / BOM 剥除。"""
import re
from pathlib import Path

ROOT = Path(r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel")
LOG = ROOT / "_attic" / "check9_full_1022_2040.log"
ERR_LINE = re.compile(r"^\s*-->\s+(varix[\\/][^:]+?):(\d+):(\d+)\s*$")

def read_lines(p):
    raw = p.read_bytes().decode("utf-8", errors="replace")
    eol = "\r\n" if "\r\n" in raw else "\n"
    return raw.split(eol), eol

def write_lines(p, lines, eol):
    p.write_bytes(eol.join(lines).encode("utf-8"))

def collect():
    out = {"e0753": {}, "innerattr": {}}
    kind = None
    for raw in LOG.read_bytes().decode("utf-8", errors="replace").splitlines():
        if raw.startswith("error"):
            kind = "e0753" if "E0753" in raw else ("innerattr" if "inner attribute" in raw else None)
            continue
        m = ERR_LINE.match(raw)
        if m and kind:
            f = m.group(1).replace("\\", "/")
            out[kind].setdefault(f, set()).add(int(m.group(2)))
    return out

def fix_e0753(p, lnos):
    """item 间的 //! 行改 //。"""
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

def main():
    kinds = collect()
    for kind, fixer in (("e0753", fix_e0753), ("innerattr", fix_innerattr)):
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
