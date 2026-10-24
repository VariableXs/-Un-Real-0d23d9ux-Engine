#!/usr/bin/env python3
"""check11 残量收敛（v2）：E0252 去重 / 缺 Vec / E0716 add_dyn / 缺 vec 宏。
注意：路径正则避免反斜杠（heredoc 转义坑），用 varix[^:]+? 匹配。"""
import re
from pathlib import Path

ROOT = Path(r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel")
LOG = ROOT / "_attic" / "check11_full_1022_2100.log"
ERR_LINE = re.compile(r"^\s*-->\s+(varix[^:]+?):(\d+):(\d+)\s*$")

def read_lines(p):
    raw = p.read_bytes().decode("utf-8", errors="replace")
    eol = "\r\n" if "\r\n" in raw else "\n"
    return raw.split(eol), eol

buckets = {"dup": {}, "needvec": {}, "tmpfmt": {}, "vecmacro": {}}
kind = None
for raw in LOG.read_bytes().decode("utf-8", errors="replace").splitlines():
    if raw.startswith("error"):
        if "E0252" in raw:
            kind = "dup"
        elif "cannot find type `Vec`" in raw:
            kind = "needvec"
        elif "E0716" in raw:
            kind = "tmpfmt"
        elif "cannot find macro `vec`" in raw:
            kind = "vecmacro"
        else:
            kind = None
        continue
    m = ERR_LINE.match(raw)
    if m and kind:
        f = m.group(1).replace("\\", "/")
        buckets[kind].setdefault(f, set()).add(int(m.group(2)))

for k, v in buckets.items():
    files = len(v)
    locs = sum(len(s) for s in v.values())
    print(f"[collect {k}] files={files} locs={locs}")

# 1) E0252：删点名 use 行
n = 0
for f, lnos in buckets["dup"].items():
    p = ROOT / f
    if not p.exists():
        continue
    lines, eol = read_lines(p)
    removed = 0
    for ln in sorted(lnos, reverse=True):
        i = ln - 1
        if 0 <= i < len(lines):
            s = lines[i].strip()
            if s.startswith("use ") and s.endswith(";"):
                del lines[i]
                removed += 1
    if removed:
        p.write_bytes(eol.join(lines).encode("utf-8"))
        n += removed
print("dup removed:", n)

# 2) 缺 Vec 导入
n = 0
for f in buckets["needvec"]:
    p = ROOT / f
    if not p.exists():
        continue
    lines, eol = read_lines(p)
    if any(l.strip() == "use alloc::vec::Vec;" for l in lines):
        continue
    done = False
    for i, l in enumerate(lines):
        s = l.strip()
        if s.startswith("use ") or s.startswith("extern crate"):
            lines.insert(i, "use alloc::vec::Vec;")
            done = True
            break
    if done:
        p.write_bytes(eol.join(lines).encode("utf-8"))
        n += 1
print("vec injected:", n)

# 3) 缺 vec 宏
n = 0
for f in buckets["vecmacro"]:
    p = ROOT / f
    if not p.exists():
        continue
    lines, eol = read_lines(p)
    if any(l.strip() == "use alloc::vec;" for l in lines):
        continue
    done = False
    for i, l in enumerate(lines):
        s = l.strip()
        if s.startswith("use ") or s.startswith("extern crate"):
            lines.insert(i, "use alloc::vec;")
            done = True
            break
    if done:
        p.write_bytes(eol.join(lines).encode("utf-8"))
        n += 1
print("vec macro injected:", n)

# 4) E0716：参数带 &format! 的 .add( 转 .add_dyn(
n = 0
for f, lnos in buckets["tmpfmt"].items():
    p = ROOT / f
    if not p.exists():
        continue
    lines, eol = read_lines(p)
    changed = False
    for ln in lnos:
        i = ln - 1
        j = i
        while 0 <= j and j > i - 8:
            if ".add(" in lines[j] and ".add_dyn(" not in lines[j]:
                lines[j] = lines[j].replace(".add(", ".add_dyn(", 1)
                changed = True
                n += 1
                break
            j -= 1
    if changed:
        p.write_bytes(eol.join(lines).encode("utf-8"))
print("add_dyn converted:", n)
