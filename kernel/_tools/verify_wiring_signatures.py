#!/usr/bin/env python3
"""核验所有接线条目的函数签名。"""
import os, re
SRC = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel/varix/src"

def ret_type(path, fn):
    src = open(os.path.join(SRC, path), encoding="utf-8", errors="ignore").read()
    m = re.search(r"pub\s+fn\s+" + re.escape(fn) + r"\s*\([^)]*\)\s*->\s*([^{;]+)\{", src)
    if m: return m.group(1).strip()
    m = re.search(r"fn\s+" + re.escape(fn) + r"\s*\([^)]*\)\s*->\s*([^{;]+)\{", src)
    return m.group(1).strip() if m else None

# ---- svstar2 ----
s = open(os.path.join(SRC, "svstar2/mod.rs"), encoding="utf-8", errors="ignore").read()
block = re.search(r"// ---- 死判据接线（2026-10-22.*?\n\s*\];", s, re.S).group(0)
bad = []
for m in re.finditer(r"\(\"([^\"]+)\", (\w+)::([\w:]+)\),", block):
    tag, mod, fn = m.groups()
    fnname = fn.split("::")[-1]
    rt = ret_type("svstar2/" + mod + ".rs", fnname)
    ok = rt is not None and "CheckSet" in rt and fn.count("::") == fnname.count("") - 1 + (1 if "::checks::" in fn or True else 0)
    # 路径含 checks:: 的，函数体需在嵌套 mod 内——签名检索已覆盖（正文搜索）
    if rt is None or "CheckSet" not in rt:
        bad.append((tag, mod, fn, rt))
print("svstar2 不匹配:", len(bad))
for b in bad: print("  ", b)

# ---- cgpu ----
s = open(os.path.join(SRC, "cgpu/mod.rs"), encoding="utf-8", errors="ignore").read()
arr = re.search(r"let blocks: \[\(&'static str, fn\(\) -> CheckSet\); CGPU_SUBSET_COUNT\] = \[(.*?)\n    \];", s, re.S)
bad2 = []
for m in re.finditer(r"\(\"([^\"]+)\", (\w+)::(\w+) as fn\(\) -> CheckSet\),", arr.group(1)):
    tag, mod, fn = m.groups()
    rt = ret_type("cgpu/" + mod + ".rs", fn)
    if rt is None or "CheckSet" not in rt:
        bad2.append((tag, mod, fn, rt))
print("cgpu 不匹配:", len(bad2))
for b in bad2: print("  ", b)

# ---- unxstar ----
s = open(os.path.join(SRC, "unxstar/mod.rs"), encoding="utf-8", errors="ignore").read()
m3 = re.search(r"pub fn unxstar_blocks\(\) -> \[\(&'static str, fn\(\) -> CheckSet\); \d+\] \{\n    \[(.*?)\n    \]\n\}", s, re.S)
bad3 = []
for m in re.finditer(r"\(\"([^\"]+)\", (\w+)::(\w+) as fn\(\) -> CheckSet\),", m3.group(1)):
    tag, mod, fn = m.groups()
    rt = ret_type("unxstar/" + mod + ".rs", fn)
    if rt is None or "CheckSet" not in rt:
        bad3.append((tag, mod, fn, rt))
print("unxstar 不匹配:", len(bad3))
for b in bad3: print("  ", b)
