#!/usr/bin/env python3
"""内容驱动式 no_std 导入补齐（免疫日志行号竞态）。

对 varix/src 全部 .rs 按实际使用补四件套：
  format!( / String 类型 / .to_string() / vec!( / Vec< 类型
仅当「文件用了 X 且没有任何 use 行提供 X」时注入独立 use 行。
"""
import re
import os
from pathlib import Path

ROOT = Path(r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel/varix/src")

stats = {"format": 0, "string": 0, "tostring": 0, "vecmacro": 0, "vectype": 0}

for dirpath, dirs, files in os.walk(ROOT):
    dirs[:] = [d for d in dirs if d not in ("target", ".git")]
    for fn in files:
        if not fn.endswith(".rs"):
            continue
        p = Path(dirpath) / fn
        try:
            raw = p.read_bytes().decode("utf-8", errors="replace")
        except Exception:
            continue
        eol = "\r\n" if "\r\n" in raw else "\n"
        lines = raw.split(eol)
        # 现有 use 面摘要
        uses = "\n".join(l for l in lines if l.strip().startswith("use "))
        injects = []

        def has(tok):
            return tok in uses

        if "format!(" in raw and not has("format"):
            injects.append("use alloc::format;")
        # String 作为类型出现（粗判：String 后跟非小写字母，或 ::new 等）
        if re.search(r"\bString\b", raw) and not has("String"):
            injects.append("use alloc::string::String;")
        if ".to_string()" in raw and not has("ToString"):
            injects.append("use alloc::string::ToString;")
        if re.search(r"\bvec!", raw) and not re.search(r"use\s+(alloc::)?(crate::alloc::)?vec\s*;", uses) and "vec," not in uses.replace("vec::", ""):
            injects.append("use alloc::vec;")
        if re.search(r"\bVec<", raw) and not has("Vec"):
            injects.append("use alloc::vec::Vec;")

        if not injects:
            continue
        # 注入点：首个 use/extern crate 行前；否则首个非注释行前
        at = None
        for i, l in enumerate(lines):
            s = l.strip()
            if s.startswith("use ") or s.startswith("extern crate"):
                at = i
                break
        if at is None:
            for i, l in enumerate(lines):
                s = l.strip()
                if s and not s.startswith("//") and not s.startswith("#!"):
                    at = i
                    break
        if at is None:
            at = len(lines)
        for k, stmt in enumerate(injects):
            lines.insert(at + k, stmt)
        p.write_bytes(eol.join(lines).encode("utf-8"))
        for stmt in injects:
            key = ("format" if "format" in stmt else
                   "vecmacro" if stmt.endswith("vec;") else
                   "vectype" if "vec::Vec" in stmt else
                   "string" if stmt.endswith("String;") else "tostring")
            stats[key] += 1

print(stats)
