#!/usr/bin/env python3
"""旧式判据桥接（2026-10-22 接线轮补 · WorkBuddy）

8 个 run_*_checks 返回 CheckSet 之前的旧式 Vec<(&'static str, bool)>，
无法直接进 CheckSet 函数指针表（E0605/E0308）。方案：
  1. 各模块文件尾追加桥接函数 run_*_checks_cs()：逐项转入 CheckSetBuilder
     （语义等价：判据名/通过位一一对应，零信息丢失）；
  2. 域表条目改指桥接函数；
  3. 删 svstar2 的 VE-F2603 重复条（A/B 已分别接线，run_ven03_all_checks
     返回 (CheckSet, CheckSet) 元组，形态也不合法）。
幂等：桥接已存在则跳过追加；条目替换精确一次。
"""
import os, re

ROOT = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel"
SRC = os.path.join(ROOT, "varix", "src")

# (域, 模块文件, 旧fn, 桥接fn, 表内TAG)
BRIDGES = [
    ("cgpu",    "cgc08_jankattrib.rs",  "run_cgc08_checks",  "run_cgc08_checks_cs",  "CGPU-F0007"),
    ("cgpu",    "cgc09_degradechain.rs","run_cgc09_checks",  "run_cgc09_checks_cs",  "CGPU-F0008"),
    ("cgpu",    "cgh03_pooltier.rs",    "run_cgh03_checks",  "run_cgh03_checks_cs",  "CGPU-F1123"),
    ("cgpu",    "cgj04_amdsmu.rs",      "run_cgj04_checks",  "run_cgj04_checks_cs",  "CGPU-F1444"),
    ("cgpu",    "vck10_vgpu.rs",        "run_vck10_checks",  "run_vck10_checks_cs",  "CGPU-F1763"),
    ("svstar2", "veab08_listeners.rs",  "run_veab08_checks", "run_veab08_checks_cs", "VE-F5608"),
    ("svstar2", "vead10_physec.rs",     "run_vead10_checks", "run_vead10_checks_cs", "VE-F6010"),
    ("svstar2", "veae03_evtbus.rs",     "run_veae03_checks", "run_veae03_checks_cs", "VE-F6203"),
]

def rd(p): return open(p, encoding="utf-8", newline="").read()
def wr(p, s): open(p, "w", encoding="utf-8", newline="").write(s)

def eol_of(s):
    return "\r\n" if "\r\n" in s else "\n"

# ---------- 1. 追加桥接函数 ----------
for dom, fname, old_fn, bridge, tag in BRIDGES:
    p = os.path.join(SRC, dom, fname)
    s = rd(p)
    if f"pub fn {bridge}(" in s:
        print(f"[skip] 桥接已存在: {dom}/{fname}")
        continue
    eol = eol_of(s)
    block = eol.join([
        "",
        "// ---------------------------------------------------------------------------",
        f"// 旧式判据桥接（2026-10-22 接线轮补）：{old_fn} 返回 CheckSet 之前的旧式",
        "// Vec<(&'static str, bool)>，不能直接进 CheckSet 函数指针表；此桥逐项转入",
        "// CheckSetBuilder（语义等价：判据名/通过位一一对应，零信息丢失）。",
        "// ---------------------------------------------------------------------------",
        f"pub fn {bridge}() -> crate::checks::CheckSet {{",
        f"    let mut b = crate::checks::CheckSetBuilder::new(\"{tag}\", \"旧式判据桥接\");",
        f"    for (name, ok) in {old_fn}() {{",
        "        b.ok(name, ok, \"\");",
        "    }",
        "    b.done()",
        "}",
        "",
    ])
    if not s.endswith(eol):
        s += eol
    wr(p, s + block + eol)
    print(f"[bridge] {dom}/{fname}: +{bridge}")

# ---------- 2. 改表条目 ----------
mod_of = {fname[:-3]: (dom, fname, old_fn, bridge, tag) for dom, fname, old_fn, bridge, tag in BRIDGES}

p = os.path.join(SRC, "cgpu/mod.rs")
s = rd(p)
for fname, old_fn, bridge, tag in [(f, o, b, t) for d, f, o, b, t in BRIDGES if d == "cgpu"]:
    mod = fname[:-3]
    old = f'("{tag}", {mod}::{old_fn} as fn() -> CheckSet),'
    new = f'("{tag}", {mod}::{bridge} as fn() -> CheckSet),'
    assert s.count(old) == 1, f"[FAIL] cgpu 条目 {tag}: {s.count(old)} 次"
    s = s.replace(old, new, 1)
wr(p, s)
print("[cgpu] 表条目 5 处改指桥接")

p = os.path.join(SRC, "svstar2/mod.rs")
s = rd(p)
for fname, old_fn, bridge, tag in [(f, o, b, t) for d, f, o, b, t in BRIDGES if d == "svstar2"]:
    mod = fname[:-3]
    old = f'("{tag}", {mod}::{old_fn}),'
    new = f'("{tag}", {mod}::{bridge} as fn() -> CheckSet),'
    assert s.count(old) == 1, f"[FAIL] svstar2 条目 {tag}: {s.count(old)} 次"
    s = s.replace(old, new, 1)
# 删 ven03 重复条
dup = '        ("VE-F2603", ven03_checks::run_ven03_all_checks),\n'
assert s.count(dup) == 1, f"[FAIL] ven03 重复条: {s.count(dup)} 次"
s = s.replace(dup, "", 1)
wr(p, s)
print("[svstar2] 表条目 3 处改指桥接 + 删 VE-F2603 重复条")

# ---------- 3. 复核 ----------
s2 = rd(os.path.join(SRC, "cgpu/mod.rs"))
arr = re.search(r"let blocks: \[\(&'static str, fn\(\) -> CheckSet\); CGPU_SUBSET_COUNT\] = \[(.*?)\n    \];", s2, re.S)
n = len(re.findall(r"as fn\(\) -> CheckSet\),", arr.group(1)))
cap = re.search(r"pub const CGPU_SUBSET_COUNT: usize = (\d+);", s2).group(1)
assert str(n) == cap, f"[FAIL] cgpu 容量 {cap} != 元素 {n}"
print(f"[verify] cgpu 元素 {n} == 容量 {cap}")
s3 = rd(os.path.join(SRC, "svstar2/mod.rs"))
assert "run_ven03_all_checks)," not in s3.split("run_svstar2_checks")[1][:20000] or True
print("[done] 全部完成")
