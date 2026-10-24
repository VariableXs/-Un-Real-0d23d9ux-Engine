#!/usr/bin/env python3
"""死判据批量接线（2026-10-22 · AI-WB）

把《死判据清单-2026-10-22.txt》里 299 条「定义后全仓零调用」的判据
接线进各自域聚合入口，使其真正参与闭环。

策略（精确字符串替换 + 计数复核 + 备份）：
  1. 备份 4 个被改文件到 _attic/wire-backup-2026-10-22/
  2. cgpu/mod.rs  ：折叠数组改函数指针表（unxstar 先例）+ 补 33 条 + 补 13 个 pub mod
  3. unxstar/mod.rs：unxstar_blocks 追加条目 + 补齐 pub mod + 数组容量同步
  4. svstar2/mod.rs：run_svstar2_checks 追加条目 + 补齐 pub mod + 数组容量同步
  5. robust.rs    ：13 个顶层/单文件域死判据直挂 352 表 + 容量同步
  6. 全程断言：每个插入都成功、每处数组容量 == 实际元素数
"""
import os, re, shutil, sys

ROOT = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel"
SRC = os.path.join(ROOT, "varix", "src")
LIST = os.path.join(ROOT, "死判据清单-2026-10-22.txt")
BK = os.path.join(ROOT, "_attic", "wire-backup-2026-10-22")

log = []
def say(msg):
    print(msg)
    log.append(msg)

# ---------- 1. 读清单 ----------
dead = []  # (fn_name, relpath 如 cgpu/cgc03_budgetaccount.rs)
for ln in open(LIST, encoding="utf-8").read().splitlines()[3:]:
    m = re.match(r"(run_\w+)\s+->\s+\.(.+)", ln)
    if m:
        dead.append((m.group(1), m.group(2).replace("\\", "/").lstrip("/")))
say(f"[load] 死判据 {len(dead)} 条")

def tag_of(relpath):
    """从文件 //! 横幅提取锚点号（UNX-F/VE-F/CGPU-F 开头）。"""
    head = open(os.path.join(SRC, relpath), encoding="utf-8", errors="ignore").read(4000)
    m = re.search(r"\b((?:UNX|VE|CGPU)-F\w+)", head)
    return m.group(1) if m else None

def modname(relpath):
    return os.path.basename(relpath)[:-3]

def banner_of(relpath):
    first = open(os.path.join(SRC, relpath), encoding="utf-8", errors="ignore").readline().strip()
    return first[:180]

# ---------- 2. 备份 ----------
os.makedirs(BK, exist_ok=True)
TOUCH = ["cgpu/mod.rs", "unxstar/mod.rs", "svstar2/mod.rs", "robust.rs"]
for p in TOUCH:
    shutil.copy2(os.path.join(SRC, p), os.path.join(BK, os.path.basename(p)))
say(f"[backup] 4 个 mod.rs -> {BK}")

def rd(p): return open(p, encoding="utf-8", newline="").read()
def wr(p, s): open(p, "w", encoding="utf-8", newline="").write(s)

def sub_once(src, old, new, label):
    """精确一次替换；old 不存在或多次出现都报错。"""
    n = src.count(old)
    assert n == 1, f"[FAIL] {label}: 出现 {n} 次（应为 1）"
    return src.replace(old, new, 1)

def insert_before_close(src, anchor_regex, indent, entries, label):
    """在 anchor 匹配的函数里找第一个独立 `];` 行前插入 entries。"""
    m = re.search(anchor_regex, src)
    assert m, f"[FAIL] {label}: 找不到锚点 {anchor_regex}"
    start = m.end()
    close = re.search(r"\n" + indent + r"\];", src[start:])
    assert close, f"[FAIL] {label}: 找不到数组收口 ];"
    pos = start + close.start()
    return src[:pos] + "\n" + entries.rstrip("\n") + src[pos:]

def append_pubmods(src, mods, label):
    """文件尾追加 pub mod 声明（带横幅注释）。"""
    if not mods:
        return src
    block = "\n// ---- 死判据补登记（2026-10-22 接线轮：模块早已实现但从未入链，等于从未参与编译与验收）----\n"
    for rel, name in mods:
        block += f"// {banner_of(rel)}\npub mod {name};\n"
    if not src.endswith("\n"):
        src += "\n"
    return src + block

# ---------- 3. 分域分组 ----------
by_domain = {}
top_level = []  # (fn, relpath) —— 单文件域，直挂 robust.rs
for fn, rel in dead:
    d = os.path.dirname(rel)
    if d in ("",):
        top_level.append((fn, rel))
    else:
        by_domain.setdefault(d, []).append((fn, rel))

# 顶层单文件域里的「目录 mod.rs 内函数」也直挂 robust（fs/mod.rs、task/mod.rs）
for d in list(by_domain):
    intra = [(fn, rel) for fn, rel in by_domain[d] if rel.endswith("mod.rs")]
    if intra:
        top_level += intra
        by_domain[d] = [x for x in by_domain[d] if not x[1].endswith("mod.rs")]
        if not by_domain[d]:
            del by_domain[d]

say("[group] 域分组: " + ", ".join(f"{k}={len(v)}" for k, v in sorted(by_domain.items()))
    + f"; 顶层/直挂 robust={len(top_level)}")

# ---------- 4. cgpu ----------
if "cgpu" in by_domain:
    items = by_domain.pop("cgpu")
    p = os.path.join(SRC, "cgpu/mod.rs")
    s = rd(p)

    # 4a. 折叠数组 → 函数指针表（unxstar 先例，防 92×4.5KB 直排撑栈）
    s = sub_once(s,
        "let blocks: [(&'static str, CheckSet); CGPU_SUBSET_COUNT] = [",
        "let blocks: [(&'static str, fn() -> CheckSet); CGPU_SUBSET_COUNT] = [",
        "cgpu 数组类型改函数指针")
    # 逐元素去调用括号，改 as fn() -> CheckSet
    lines = s.split("\n")
    bad = []
    for i, ln in enumerate(lines):
        m = re.match(r"^(\s*)\(\"([^\"]+)\", (\w+::run_\w+_checks)\(\)\),\s*$", ln)
        if m:
            lines[i] = f'{m.group(1)}("{m.group(2)}", {m.group(3)} as fn() -> CheckSet),'
        elif re.match(r"^\s*\"[^\"]+\", run_\w+", ln) or re.search(r"::run_\w+_checks\(\)\)", ln):
            if ln.strip().startswith("//"):
                continue
            bad.append(ln)
    assert not bad, "[FAIL] cgpu 有未能机械转换的元素行:\n" + "\n".join(bad[:5])
    s = "\n".join(lines)
    # 消费循环改为即用即弃
    s = sub_once(s,
        "for (tag, sub) in blocks.iter() {\n        let passed = sub.all_passed() && !sub.truncated();",
        "for (tag, f) in blocks.iter() {\n        // 即用即弃：同一时刻栈上只存活一个子集 CheckSet（unxstar 同款手法）。\n        let sub = f();\n        let passed = sub.all_passed() && !sub.truncated();",
        "cgpu 消费循环")

    # 4b. 追加 33 条（标签取文件横幅）
    entries = []
    for fn, rel in items:
        tag = tag_of(rel)
        assert tag, f"[FAIL] cgpu/{modname(rel)} 无横幅标签"
        entries.append(f'        ("{tag}", {modname(rel)}::{fn} as fn() -> CheckSet),')
    say(f"[cgpu] 待接线 {len(entries)} 条: " + ", ".join(e.split('"')[1] for e in entries[:5]) + " ...")
    s = insert_before_close(s, r"pub fn run_cgpu_checks\(\)", "    ", "\n".join(entries), "cgpu 数组追加")

    # 4c. 缺声明的补 pub mod（文件尾）
    p_mods = os.path.join(SRC, "cgpu/mod.rs")
    missing = [(rel, modname(rel)) for fn, rel in items
               if not re.search(r"mod\s+" + re.escape(modname(rel)) + r"\s*;", s)]
    s = append_pubmods(s, missing, "cgpu")
    say(f"[cgpu] 补 pub mod {len(missing)} 个: " + ", ".join(m[1] for m in missing[:5]) + " ...")

    # 4d. 容量锁：59 -> 59+len
    old_n, add_n = 59, len(items)
    s = sub_once(s, "pub const CGPU_SUBSET_COUNT: usize = 59;",
                 f"pub const CGPU_SUBSET_COUNT: usize = {old_n + add_n};", "CGPU_SUBSET_COUNT")
    wr(p, s)
    # 复核
    s2 = rd(p)
    arr = re.search(r"let blocks: \[\(&'static str, fn\(\) -> CheckSet\); CGPU_SUBSET_COUNT\] = \[(.*?)\n    \];", s2, re.S)
    n = len(re.findall(r"as fn\(\) -> CheckSet\),", arr.group(1)))
    assert n == old_n + add_n, f"[FAIL] cgpu 数组元素 {n} != {old_n+add_n}"
    say(f"[cgpu] OK：数组元素复核 {n} == {old_n+add_n}")

# ---------- 5. unxstar / svstar2 ----------
def wire_big_domain(domain, array_anchor, size_old, entry_fmt, array_re):
    items = by_domain.pop(domain)
    p = os.path.join(SRC, domain, "mod.rs")
    s = rd(p)
    entries = []
    for fn, rel in items:
        tag = tag_of(rel)
        assert tag, f"[FAIL] {domain}/{modname(rel)} 无横幅标签"
        entries.append(entry_fmt.format(tag=tag, mod=modname(rel), fn=fn))
    say(f"[{domain}] 待接线 {len(entries)} 条")
    s = insert_before_close(s, array_anchor, "    ", "\n".join(entries), f"{domain} 数组追加")
    missing = [(rel, modname(rel)) for fn, rel in items
               if not re.search(r"mod\s+" + re.escape(modname(rel)) + r"\s*;", s)]
    s = append_pubmods(s, missing, domain)
    say(f"[{domain}] 补 pub mod {len(missing)} 个")
    # 容量同步：所有 `; {size_old}]` 字面量
    pat = f"; {size_old}]"
    cnt = s.count(pat)
    assert cnt >= 1, f"[FAIL] {domain}: 找不到容量字面量 {pat}"
    s = s.replace(pat, f"; {size_old + len(items)}]")
    say(f"[{domain}] 容量 {size_old} -> {size_old + len(items)}（替换 {cnt} 处）")
    wr(p, s)
    # 复核
    s2 = rd(p)
    m = re.search(array_re, s2, re.S)
    assert m, f"[FAIL] {domain}: 复核时找不到数组"
    body = m.group(1)
    n = len(re.findall(r"^\s*\(\"", body, re.M))
    assert n == size_old + len(items), f"[FAIL] {domain}: 数组元素 {n} != {size_old+len(items)}"
    say(f"[{domain}] OK：数组元素复核 {n}")

if "unxstar" in by_domain:
    wire_big_domain(
        "unxstar",
        r"pub fn unxstar_blocks\(\)",
        69,
        '        ("{tag}", {mod}::{fn} as fn() -> CheckSet),',
        r"pub fn unxstar_blocks\(\) -> \[\(&'static str, fn\(\) -> CheckSet\); \d+\] \[(.*?)\n    \]")

if "svstar2" in by_domain:
    wire_big_domain(
        "svstar2",
        r"pub fn run_svstar2_checks\(\)",
        594,
        '        ("{tag}", {mod}::{fn}),',
        r"pub fn run_svstar2_checks\(\) -> CheckSet \{.*?\[\(&'static str, fn\(\) -> CheckSet\); \d+\] = \[(.*?)\n    \];")

# ---------- 6. 顶层/单文件域直挂 robust.rs ----------
if top_level:
    p = os.path.join(SRC, "robust.rs")
    s = rd(p)
    lines = []
    for fn, rel in top_level:
        modpath = rel[:-3].replace("/", "::")
        lines.append(f"    crate::{modpath}::{fn},")
    say("[robust] 直挂 " + str(len(lines)) + " 条: " + "; ".join(l.strip() for l in lines[:4]) + " ...")
    block = "\n".join(lines)
    # 插在 352 表收口 ]; 前
    m = re.search(r"let domains: \[fn\(\) -> CheckSet; 352\] = \[", s)
    assert m, "[FAIL] robust: 找不到 352 表起点"
    close = re.search(r"\n\s*\];", s[m.end():])
    assert close, "[FAIL] robust: 找不到 352 表收口"
    pos = m.end() + close.start()
    s = s[:pos] + "\n" + block + s[pos:]
    s = s.replace("; 352] = [", f"; {352 + len(top_level)}] = [")
    wr(p, s)
    s2 = rd(p)
    m2 = re.search(r"let domains: \[fn\(\) -> CheckSet; (\d+)\] = \[(.*?)\n\s*\];", s2, re.S)
    n = len(re.findall(r"^\s*crate::\w", m2.group(2), re.M))
    assert n == int(m2.group(1)), f"[FAIL] robust: 表项 {n} != 容量 {m2.group(1)}"
    say(f"[robust] OK：表项复核 {n} == 容量 {m2.group(1)}")

leftover = sum(len(v) for v in by_domain.values())
say(f"[done] 剩余未处理: {leftover}")
for d, v in by_domain.items():
    say("  " + d + ": " + ", ".join(fn for fn, _ in v))
open(os.path.join(BK, "wire-log.txt"), "w", encoding="utf-8").write("\n".join(log))
