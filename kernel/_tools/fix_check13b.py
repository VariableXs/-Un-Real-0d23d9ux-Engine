#!/usr/bin/env python3
"""check13b：E0521 add_dyn / 非 ASCII 字节串 / 撞车 Vec 独立导入清除。"""
import re
import os
from pathlib import Path

ROOT = Path(r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel")
LOG = ROOT / "_attic" / "check13_full_1022_2125.log"
ERR_LINE = re.compile(r"^\s*-->\s+(varix[^:]+?):(\d+):(\d+)\s*$")

# 1) E0521：chk 包装函数 add → add_dyn
files_e0521 = set()
kind = None
for raw in LOG.read_bytes().decode("utf-8", errors="replace").splitlines():
    if raw.startswith("error"):
        kind = "E0521" if "E0521" in raw else None
        continue
    m = ERR_LINE.match(raw)
    if m and kind == "E0521":
        files_e0521.add(m.group(1).replace("\\", "/"))
print("e0521 files:", len(files_e0521))

n = 0
for f in files_e0521:
    p = ROOT / f
    if not p.exists():
        continue
    raw = p.read_bytes().decode("utf-8", errors="replace")
    eol = "\r\n" if "\r\n" in raw else "\n"
    if "set.add(label, ok, detail);" in raw:
        raw = raw.replace("set.add(label, ok, detail);", "set.add_dyn(label, ok, detail);")
        p.write_bytes(raw.encode("utf-8"))
        n += 1
print("add_dyn chk fixed:", n)

# 2) 非 ASCII 字节串 → "…".as_bytes()
#    找 check13 日志里报该错的文件，逐个替换 b"非ASCII" 为 "…".as_bytes()
files_ascii = set()
kind = None
for raw in LOG.read_bytes().decode("utf-8", errors="replace").splitlines():
    if raw.startswith("error"):
        kind = "ascii" if "non-ASCII character in byte string" in raw else None
        continue
    m = ERR_LINE.match(raw)
    if m and kind == "ascii":
        files_ascii.add(m.group(1).replace("\\", "/"))
print("ascii files:", len(files_ascii))

n = 0
for f in files_ascii:
    p = ROOT / f
    if not p.exists():
        continue
    raw = p.read_bytes().decode("utf-8", errors="replace")
    eol = "\r\n" if "\r\n" in raw else "\n"
    def repl(m):
        inner = m.group(1)
        if all(ord(c) < 128 for c in inner):
            return m.group(0)
        return '"' + inner + '".as_bytes()'
    new = re.sub(r'b"([^"]*)"', repl, raw)
    if new != raw:
        p.write_bytes(new.encode("utf-8"))
        n += 1
print("ascii byte-literal fixed:", n)

# 3) 清除撞车：文件里既有我方独立 use alloc::vec::Vec; 又有其它 use 行提供 Vec
n = 0
for dirpath, dirs, files in os.walk(ROOT / "varix" / "src"):
    dirs[:] = [d for d in dirs if d not in ("target", ".git")]
    for fn in files:
        if not fn.endswith(".rs"):
            continue
        p = Path(dirpath) / fn
        try:
            raw = p.read_bytes().decode("utf-8", errors="replace")
        except Exception:
            continue
        lines = raw.split("\n")
        std = [i for i, l in enumerate(lines) if l.strip() == "use alloc::vec::Vec;"]
        if len(std) != 1:
            continue
        i0 = std[0]
        others = [l for j, l in enumerate(lines)
                  if j != i0 and l.strip().startswith("use ") and "Vec" in l]
        if others:
            eol = "\r\n" if "\r\n" in raw else "\n"
            del lines[i0]
            p.write_bytes(eol.join(lines).encode("utf-8"))
            n += 1
print("colliding standalone Vec removed:", n)
