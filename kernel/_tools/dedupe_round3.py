#!/usr/bin/env python3
"""第三轮去重：识别 crate::alloc 前缀与分组 self 的宏覆盖。"""
import os, re
SRC = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel/varix/src"
ROOT = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel"
targets = set()
for ln in open(os.path.join(ROOT, "死判据清单-2026-10-22.txt"), encoding="utf-8").read().splitlines()[3:]:
    m = re.match(r"(run_\w+)\s+->\s+\.(.+)", ln)
    if m:
        targets.add(m.group(2).replace("\\", "/").lstrip("/"))
IMPL = ["cgpu/cga11_power_sched.rs", "cgpu/cgp03_sampling.rs", "cgpu/cgp04_pipeline.rs",
        "cgpu/cgp05_tiers.rs", "cgpu/cgp06_query.rs", "cgpu/cgp07_jmerge.rs", "cgpu/cgp09_privacy.rs",
        "svstar2/veai01_aiarch.rs", "svstar2/veb230_firmstate.rs", "svstar2/vee13_examples.rs",
        "svstar2/vef06_apng.rs"]
targets = [t for t in sorted(targets) if os.path.exists(os.path.join(SRC, t))] + IMPL
MY_MACRO = {"use alloc::vec;", "use alloc::format;"}

def covers_macro(others, item):
    for ln in others:
        m = re.match(r"\s*use\s+(crate::)?alloc::(\w+)::\{([^}]*)\}\s*;", ln)
        if m:
            mod = m.group(2)
            items = [i.strip() for i in m.group(3).split(",")]
            if mod == item and ("self" in items or item in items):
                return True
            if item in items:
                return True
            continue
        m = re.match(r"\s*use\s+(crate::)?alloc::(\w+)\s*;", ln)
        if m and m.group(2) == item:
            return True
    return False

fixed = 0
for rel in targets:
    p = os.path.join(SRC, rel)
    if not os.path.exists(p):
        continue
    raw = open(p, encoding="utf-8", newline="").read()
    crlf = "\r\n" in raw
    s = raw.replace("\r\n", "\n")
    lines = s.split("\n")
    mine = [i for i, ln in enumerate(lines) if ln.strip() in MY_MACRO]
    if not mine:
        continue
    others = [ln for i, ln in enumerate(lines) if i not in mine]
    drop = set()
    for i in mine:
        item = "vec" if "vec;" in lines[i] else "format"
        if covers_macro(others, item):
            drop.add(i)
    if drop:
        out = [ln for i, ln in enumerate(lines) if i not in drop]
        s2 = "\n".join(out)
        if crlf:
            s2 = s2.replace("\n", "\r\n")
        open(p, "w", encoding="utf-8", newline="").write(s2)
        fixed += 1
print("第三轮去重:", fixed, "个文件")
