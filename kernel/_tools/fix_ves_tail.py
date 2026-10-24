#!/usr/bin/env python3
"""ves156/157 尾块补 return 表达式（E0308: expected CheckSet, found ()）。"""
from pathlib import Path

for f in ["varix/src/svstar2/ves156_sri18n.rs", "varix/src/svstar2/ves157_s03integ.rs"]:
    p = Path(r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel") / f
    raw = p.read_bytes().decode("utf-8", errors="replace")
    eol = "\r\n" if "\r\n" in raw else "\n"
    lines = raw.split(eol)
    has_set = any("let mut set" in l for l in lines)
    j = len(lines) - 1
    while j >= 0 and lines[j].strip() == "":
        j -= 1
    if lines[j].strip() == "}" and has_set and lines[j-1].strip() != "set":
        lines.insert(j, "    set")
        p.write_bytes(eol.join(lines).encode("utf-8"))
        print(f, "-> tail 'set' inserted")
    else:
        print(f, "-> tail:", repr(lines[j].strip()[:40]), "has_set:", has_set)
