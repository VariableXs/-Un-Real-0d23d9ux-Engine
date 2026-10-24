#!/usr/bin/env python3
"""阶段4：补实现模块声明 + 新入链文件 no_std 导入自检注入。

1. cgpu/svstar2 mod.rs 补 11 个实现模块 pub mod（E0432 修复）。
2. 对本轮新入链的全部文件扫描 vec!/format!/Vec/String/ToString/Box 使用，
   缺什么注入什么（与 G3 三件套同口径），插入点为头部 //! 横幅之后。
"""
import os, re

SRC = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel/varix/src"

# ---- 1) 实现模块声明 ----
IMPL = {
    "cgpu/mod.rs": ["cga11_power_sched", "cgp03_sampling", "cgp04_pipeline",
                     "cgp05_tiers", "cgp06_query", "cgp07_jmerge", "cgp09_privacy"],
    "svstar2/mod.rs": ["veai01_aiarch", "veb230_firmstate", "vee13_examples", "vef06_apng"],
}
for modfile, mods in IMPL.items():
    p = os.path.join(SRC, modfile)
    s = open(p, encoding="utf-8", newline="").read()
    new = [m for m in mods if not re.search(r"mod\s+" + m + r"\s*;", s)]
    if new:
        block = "\n// ---- 实现模块补登记（2026-10-22：判据接线时发现实现兄弟模块同样未入链）----\n"
        block += "".join(f"pub mod {m};\n" for m in new)
        if not s.endswith("\n"):
            s += "\n"
        s += block
        open(p, "w", encoding="utf-8", newline="").write(s)
    print(f"[decl] {modfile}: 补 {len(new)} / {len(mods)}")

# ---- 2) no_std 导入注入 ----
# 本轮新入链文件 = 各域补登记块里的 pub mod（按标记行收集）
targets = []
for modfile in ["cgpu/mod.rs", "unxstar/mod.rs", "svstar2/mod.rs"]:
    d = os.path.dirname(modfile)
    s = open(os.path.join(SRC, modfile), encoding="utf-8", errors="ignore").read()
    in_block = False
    for ln in s.splitlines():
        if "补登记" in ln and "2026-10-22" in ln:
            in_block = True
            continue
        if in_block:
            m = re.match(r"pub mod (\w+);", ln.strip())
            if m:
                targets.append(os.path.join(d, m.group(1) + ".rs"))
            elif ln.strip().startswith("pub mod"):
                continue
            else:
                in_block = False
targets = [t for t in targets if os.path.exists(os.path.join(SRC, t))]
print(f"[scan] 新入链文件 {len(targets)} 个")

NEED = [
    (r"\bvec!\s*[([\{]", "alloc::vec", "use alloc::vec;"),
    (r"\bformat!\s*\(", "alloc::format", "use alloc::format;"),
    (r"\bVec\s*<[A-Za-z_(]", "alloc::vec::Vec", "use alloc::vec::Vec;"),
    (r"\bString\b", "alloc::string::String", "use alloc::string::String;"),
    (r"\.to_string\(\)", "alloc::string::ToString", "use alloc::string::ToString;"),
    (r"\.to_owned\(\)", "alloc::borrow::ToOwned", "use alloc::borrow::ToOwned;"),
    (r"\bBox\s*<[A-Za-z_(]", "alloc::boxed::Box", "use alloc::boxed::Box;"),
]

fixed = 0
for rel in targets:
    p = os.path.join(SRC, rel)
    s = open(p, encoding="utf-8", newline="").read()
    # 已有 use 树（粗粒度：出现 use alloc::xxx 即视为已引入该路径）
    have = set(re.findall(r"use\s+(alloc::[\w:]+)", s))
    need_lines = []
    for pat, path, stmt in NEED:
        if re.search(pat, s) and not any(h.startswith(path) for h in have):
            need_lines.append(stmt)
    if not need_lines:
        continue
    lines = s.split("\n")
    insert_at = 0
    for i, ln in enumerate(lines):
        if ln.strip().startswith("//!"):
            insert_at = i + 1
        elif insert_at and ln.strip() and not ln.strip().startswith("//!") and i < 40:
            break
    block = "\n".join(need_lines)
    lines.insert(insert_at, block)
    open(p, "w", encoding="utf-8", newline="").write("\n".join(lines))
    fixed += 1
    print(f"  [inject] {rel}: {len(need_lines)} 条")
print(f"[done] 注入 {fixed} 个文件")
