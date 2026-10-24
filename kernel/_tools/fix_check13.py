#!/usr/bin/env python3
"""check13 残量：E0252 删我方注入行（保留原组导入）/ 缺 Vec / 缺 vec 宏。"""
import re
from pathlib import Path

ROOT = Path(r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel")
LOG = ROOT / "_attic" / "check13_full_1022_2125.log"
ERR_LINE = re.compile(r"^\s*-->\s+(varix[^:]+?):(\d+):(\d+)\s*$")
MINE = {
    "use alloc::format;", "use alloc::string::String;", "use alloc::string::ToString;",
    "use alloc::vec;", "use alloc::vec::Vec;",
}

def read_lines(p):
    raw = p.read_bytes().decode("utf-8", errors="replace")
    eol = "\r\n" if "\r\n" in raw else "\n"
    return raw.split(eol), eol

buckets = {"dup": {}, "needvec": {}, "vecmacro": {}}
kind = None
prev_line = None
for raw in LOG.read_bytes().decode("utf-8", errors="replace").splitlines():
    if raw.startswith("error"):
        if "E0252" in raw:
            kind = "dup"
        elif "cannot find type `Vec`" in raw:
            kind = "needvec"
        elif "cannot find macro `vec`" in raw:
            kind = "vecmacro"
        else:
            kind = None
        continue
    m = ERR_LINE.match(raw)
    if m and kind:
        f = m.group(1).replace("\\", "/")
        buckets[kind].setdefault(f, set()).add(int(m.group(2)))

print({k: (len(v), sum(len(s) for s in v.values())) for k, v in buckets.items()})

# E0252：在报错行上方 12 行内找我方注入串删除
n = 0
for f, lnos in buckets["dup"].items():
    p = ROOT / f
    if not p.exists():
        continue
    lines, eol = read_lines(p)
    drop = set()
    for ln in lnos:
        i = ln - 1
        for j in range(max(0, i - 12), i + 1):
            if lines[j].strip() in MINE:
                drop.add(j)
                break
    if drop:
        for j in sorted(drop, reverse=True):
            del lines[j]
        p.write_bytes(eol.join(lines).encode("utf-8"))
        n += len(drop)
print("my-inject dup removed:", n)

# 缺 Vec / vec 宏
def inject(f, stmt):
    p = ROOT / f
    if not p.exists():
        return 0
    lines, eol = read_lines(p)
    if any(l.strip() == stmt for l in lines):
        return 0
    for i, l in enumerate(lines):
        s = l.strip()
        if s.startswith("use ") or s.startswith("extern crate"):
            lines.insert(i, stmt)
            p.write_bytes(eol.join(lines).encode("utf-8"))
            return 1
    return 0

n = sum(inject(f, "use alloc::vec::Vec;") for f in buckets["needvec"])
print("vec injected:", n)
n = sum(inject(f, "use alloc::vec;") for f in buckets["vecmacro"])
print("vec macro injected:", n)
