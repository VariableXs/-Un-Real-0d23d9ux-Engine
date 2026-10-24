#!/usr/bin/env python3
"""死判据批量接线 v2（2026-10-22 深夜 · WorkBuddy）

针对《死判据清单-2026-10-22-晚.txt》剩余 92 条（cgpu 55 / unxstar 27 /
svstar2 6 / vestar 4）。v1（wire_dead_checks.py）容量数字写死旧批次，
不能复用；v2 一律**重数实际元素**同步容量，且幂等（已接线项自动跳过）。

策略：
  1. 备份被改文件到 _attic/wire2-backup-2026-10-22/
  2. cgpu   ：blocks 函数指针表追加条目 + 补 pub mod + 重数容量
  3. unxstar：unxstar_blocks 追加条目 + 补 pub mod + 重数容量
  4. svstar2：run_svstar2_checks 内数组追加条目 + 补 pub mod + 重数容量
  5. vestar + 顶层/单文件域：直挂 robust.rs domains 表 + 重数容量
  6. 全程断言 + 写后复核；--dry-run 只报告不落盘
"""
import os, re, shutil, sys

ROOT = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel"
SRC = os.path.join(ROOT, "varix", "src")
LIST = os.path.join(ROOT, "死判据清单-2026-10-22-晚.txt")
BK = os.path.join(ROOT, "_attic", "wire2-backup-2026-10-22")
DRY = "--dry-run" in sys.argv
log = []

def say(msg):
    print(msg)
    log.append(msg)

# ---------- 1. 读清单 ----------
dead = []
for ln in open(LIST, encoding="utf-8").read().splitlines():
    m = re.match(r"(run_\w+)\s+->\s+\.(.+)", ln)
    if m:
        dead.append((m.group(1), m.group(2).replace("\\", "/").lstrip("/")))
say(f"[load] 死判据 {len(dead)} 条{'（dry-run）' if DRY else ''}")

def tag_of(relpath):
    head = open(os.path.join(SRC, relpath), encoding="utf-8", errors="ignore").read(4000)
    m = re.search(r"\b((?:UNX|VE|CGPU)-F\w+)", head)
    return m.group(1) if m else None

def modname(relpath):
    return os.path.basename(relpath)[:-3]

def rd(p): return open(p, encoding="utf-8", newline="").read()
def wr(p, s):
    if DRY:
        say(f"  [dry] 不写 {os.path.relpath(p, ROOT)}")
        return
    open(p, "w", encoding="utf-8", newline="").write(s)

def backup(*relps):
    os.makedirs(BK, exist_ok=True)
    for r in relps:
        shutil.copy2(os.path.join(SRC, r), os.path.join(BK, r.replace("/", "_")))

def append_to_array(src, anchor_re, close_pat, entries, label):
    """在 anchor 后第一个匹配 close_pat 的收口前插入 entries。"""
    m = re.search(anchor_re, src)
    assert m, f"[FAIL] {label}: 找不到锚点"
    close = re.search(close_pat, src[m.end():])
    assert close, f"[FAIL] {label}: 找不到数组收口"
    pos = m.end() + close.start()
    return src[:pos] + "\n" + "\n".join(entries).rstrip("\n") + src[pos:]

def missing_pubmods(src, items):
    return [(rel, modname(rel)) for fn, rel in items
            if not re.search(r"mod\s+" + re.escape(modname(rel)) + r"\s*;", src)]

def append_pubmods(src, mods):
    if not mods:
        return src
    block = "\n// ---- 死判据补登记（2026-10-22 接线 v2：模块已实现但从未入链）----\n"
    for rel, name in mods:
        first = open(os.path.join(SRC, rel), encoding="utf-8", errors="ignore").readline().strip()
        block += f"// {first[:170]}\npub mod {name};\n"
    if not src.endswith("\n"):
        src += "\n"
    return src + block

def sync_cap_by_recount(src, cap_re, n, label):
    """把 cap_re 匹配到的容量字面量替换为重数值 n（None 则跳过——切片无数组容量）。"""
    if cap_re is None:
        say(f"[{label}] 切片形态，无容量锁，跳过同步")
        return src
    m = re.search(cap_re, src)
    assert m, f"[FAIL] {label}: 找不到容量字面量"
    old = int(m.group(1))
    if old != n:
        say(f"[{label}] 容量 {old} -> {n}")
        src = src[:m.start(1)] + str(n) + src[m.end(1):]
    return src

# ---------- 2. 分域分组 ----------
by_domain = {}
top_level = []
for fn, rel in dead:
    d = os.path.dirname(rel)
    if d == "" or rel.endswith("mod.rs"):
        top_level.append((fn, rel))
    else:
        by_domain.setdefault(d, []).append((fn, rel))
say("[group] " + ", ".join(f"{k}={len(v)}" for k, v in sorted(by_domain.items()))
    + f"; 直挂 robust={len(top_level)}")

# ---------- 3. cgpu ----------
if "cgpu" in by_domain:
    items = by_domain.pop("cgpu")
    p = os.path.join(SRC, "cgpu/mod.rs")
    s = rd(p)
    items = [(fn, rel) for fn, rel in items
             if f"{modname(rel)}::{fn}" not in s]  # 幂等
    if not items:
        say("[cgpu] 全部已接线，跳过")
    else:
        backup("cgpu/mod.rs")
        entries, skips = [], []
        for fn, rel in items:
            tag = tag_of(rel)
            if not tag:
                skips.append(rel); continue
            entries.append(f'        ("{tag}", {modname(rel)}::{fn} as fn() -> CheckSet),')
        if skips:
            say(f"[cgpu] 无横幅标签跳过 {len(skips)}: {', '.join(skips)}")
        say(f"[cgpu] 接线 {len(entries)} 条")
        s = append_to_array(s,
            r"let blocks: \[\(&'static str, fn\(\) -> CheckSet\); CGPU_SUBSET_COUNT\] = \[",
            r"\n    \];", entries, "cgpu blocks")
        s = append_pubmods(s, missing_pubmods(s, items))
        wr(p, s)
        # 复核 + 容量重数同步
        s2 = rd(p)
        arr = re.search(r"let blocks: \[\(&'static str, fn\(\) -> CheckSet\); CGPU_SUBSET_COUNT\] = \[(.*?)\n    \];", s2, re.S)
        n = len(re.findall(r"as fn\(\) -> CheckSet\),", arr.group(1)))
        s2 = sync_cap_by_recount(s2, r"pub const CGPU_SUBSET_COUNT: usize = (\d+);", n, "cgpu")
        wr(p, s2)
        say(f"[cgpu] OK：元素复核 {n}，容量同步")

# ---------- 4. unxstar / svstar2 ----------
def wire_big_domain(domain, anchor_re, close_pat, entry_fmt, arr_re, cnt_re):
    items = by_domain.pop(domain)
    p = os.path.join(SRC, domain + "/mod.rs")
    s = rd(p)
    items = [(fn, rel) for fn, rel in items
             if f"{modname(rel)}::{fn}" not in s]
    if not items:
        say(f"[{domain}] 全部已接线，跳过")
        return
    backup(domain + "/mod.rs")
    entries, skips = [], []
    for fn, rel in items:
        tag = tag_of(rel)
        if not tag:
            skips.append(rel); continue
        entries.append(entry_fmt.format(tag=tag, mod=modname(rel), fn=fn))
    if skips:
        say(f"[{domain}] 无横幅标签跳过 {len(skips)}: {', '.join(skips)}")
    say(f"[{domain}] 接线 {len(entries)} 条")
    s = append_to_array(s, anchor_re, close_pat, entries, domain)
    s = append_pubmods(s, missing_pubmods(s, items))
    wr(p, s)
    s2 = rd(p)
    m = re.search(arr_re, s2, re.S)
    assert m, f"[FAIL] {domain}: 复核找不到数组"
    # arr_re 可能带容量捕获组（group1=数字, group2=正文）或只有正文一个组
    body = m.group(2) if m.lastindex and m.lastindex >= 2 else m.group(1)
    n = len(re.findall(r"^\s*\(\"", body, re.M))
    s2 = sync_cap_by_recount(s2, cnt_re, n, domain)
    wr(p, s2)
    say(f"[{domain}] OK：元素复核 {n}")

if "unxstar" in by_domain:
    wire_big_domain(
        "unxstar",
        r"pub fn unxstar_blocks\(\)",
        r"\n    \]",
        '        ("{tag}", {mod}::{fn} as fn() -> CheckSet),',
        r"pub fn unxstar_blocks\(\) -> \[\(&'static str, fn\(\) -> CheckSet\); (\d+)\] \{\s*\[(.*?)\n    \]",
        r"pub fn unxstar_blocks\(\) -> \[\(&'static str, fn\(\) -> CheckSet\); (\d+)\]")

if "svstar2" in by_domain:
    wire_big_domain(
        "svstar2",
        r"pub fn run_svstar2_checks\(\)",
        r"\n    \];",
        '        ("{tag}", {mod}::{fn}),',
        r"pub fn run_svstar2_checks\(\) -> CheckSet \{.*?let blocks: &\[\(&'static str, fn\(\) -> CheckSet\)\] = &\[(.*?)\n    \];",
        None)

# ---------- 5. vestar + 顶层直挂 robust.rs ----------
if top_level or "vestar" in by_domain:
    hooks = [(fn, rel) for fn, rel in top_level]
    hooks += by_domain.pop("vestar", [])
    p = os.path.join(SRC, "robust.rs")
    s = rd(p)
    hooks = [(fn, rel) for fn, rel in hooks
             if f"crate::{rel[:-3].replace('/', '::')}::{fn}" not in s]
    if hooks:
        backup("robust.rs")
        lines = [f"    crate::{rel[:-3].replace('/', '::')}::{fn}," for fn, rel in hooks]
        say("[robust] 直挂 " + str(len(lines)) + " 条")
        m = re.search(r"let domains: \[fn\(\) -> CheckSet; (\d+)\] = \[", s)
        assert m, "[FAIL] robust: 找不到 domains 表"
        close = re.search(r"\n\s*\];", s[m.end():])
        assert close, "[FAIL] robust: 找不到表收口"
        pos = m.end() + close.start()
        s = s[:pos] + "\n" + "\n".join(lines) + s[pos:]
        wr(p, s)
        s2 = rd(p)
        m2 = re.search(r"let domains: \[fn\(\) -> CheckSet; (\d+)\] = \[(.*?)\n\s*\];", s2, re.S)
        n = len(re.findall(r"^\s*crate::\w", m2.group(2), re.M))
        s2 = sync_cap_by_recount(s2, r"let domains: \[fn\(\) -> CheckSet; (\d+)\] = \[", n, "robust")
        wr(p, s2)
        say(f"[robust] OK：表项复核 {n}")

leftover = sum(len(v) for v in by_domain.values())
say(f"[done] 剩余未处理: {leftover}")
for d, v in by_domain.items():
    say("  " + d + ": " + ", ".join(fn for fn, _ in v))
if not DRY:
    os.makedirs(BK, exist_ok=True)
    open(os.path.join(BK, "wire2-log.txt"), "w", encoding="utf-8").write("\n".join(log))
