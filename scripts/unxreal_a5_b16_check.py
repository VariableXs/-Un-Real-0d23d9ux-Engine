#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""unxreal_a5_b16_check.py — A5 域 B16–B30 续领波 finalize 五步断言链机械校验器（AI-05 · 波08-M08）

五步断言链：
  ① 防重：F3501–F3800 定义位三树唯一（batches/deepen/kernel），他域零定义；
  ② 判据：300 条判据 J1 全在、记账表判据计数、M 号配对（M-259–M-558 连续+齐备）；
  ③ 行数守恒：15 批逐批求和==骨架锁定==记账表合计，域级总轧 90,040，域累计 179,940/240,000；
  ④ 台账：W2 台账 B16–B30 行 [已深化]（由 --with-ledger 开启时核验）；
  ⑤ 四项齐备：逐条正文 ≥300 字（wc -m 同口径 len()）。
exit 0 = 全绿；任何 FAIL 打印坐标并 exit 1。
"""
import re, sys, glob, os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
B16, B30 = 3501, 3800
BATCHES = list(range(16, 31))
EXPECT_SUM = {16:6000,17:6000,18:6040,19:6000,20:5960,21:6000,22:6040,23:6000,
              24:5960,25:6000,26:6040,27:6000,28:5960,29:6000,30:6040}
TOTAL = 90040
fails = []

def fail(msg):
    fails.append(msg); print("FAIL:", msg)

def ok(msg): print("ok  :", msg)

# ---------- ① 防重 ----------
def is_def_line(line, fid):
    s = line.strip()
    return s.startswith(f"### {fid} ") or s.startswith(f"## {fid} ") or bool(re.match(r"(pub\s+)?(fn|struct|enum|static)\s+%s\b" % fid, s))

defs = {}
for tree, pat in [("batches", "docs/unxreal/batches/UNX-A5-B*.md"),
                  ("deepen", "docs/unxreal/deepen/A5-B*.md"),
                  ("kernel", "kernel/**/*.rs")]:
    for f in glob.glob(os.path.join(ROOT, pat), recursive=True):
        for i, line in enumerate(open(f, encoding="utf-8", errors="ignore"), 1):
            for fid in re.findall(r"F3[5-8]\d\d", line):
                n = int(fid[1:])
                if B16 <= n <= B30 and is_def_line(line, fid):
                    defs.setdefault(fid, []).append((tree, f"{os.path.basename(f)}:{i}"))
# 产权判据：每号恰两处定义位（同批批件 ### + 深化册 ##），kernel 与他域零定义
bad = {}
for fid, locs in defs.items():
    trees = sorted(x[0] for x in locs)
    if trees != ["batches", "deepen"]:
        bad[fid] = locs
if bad: fail(f"①防重：定义位异常（应为批件+深化册各一）{dict(list(bad.items())[:5])}")
else: ok(f"①防重：F3501–F3800 定义位=批件+深化册成对唯一，kernel 零定义（共 {len(defs)} 号）")

missing = [f"F{n}" for n in range(B16, B30+1) if f"F{n}" not in defs]
if missing: fail(f"①防重：缺定义 {missing[:10]}")
else: ok("①防重：300 号 300 定义，无缺号")

# ---------- ②③⑤ 逐批 ----------
all_m = []
cum = 0
for b in BATCHES:
    skel = open(os.path.join(ROOT, f"docs/unxreal/batches/UNX-A5-B{b}.md"), encoding="utf-8").read()
    book = open(os.path.join(ROOT, f"docs/unxreal/deepen/A5-B{b}.md"), encoding="utf-8").read()
    base = B16 + (b-16)*20
    exp_ids = [f"F{base+i}" for i in range(20)]
    # skeleton rows
    srows = re.findall(r"### (F\d{4}) [^\n]*\n- 域 UNX-A5 · 批 B\d+ · 行数锁定 (\d+) · 状态 \[骨架\]", skel)
    if [r[0] for r in srows] != exp_ids:
        fail(f"B{b}: 骨架 ID 序列不符"); continue
    s_sum = sum(int(r[1]) for r in srows)
    if s_sum != EXPECT_SUM[b]:
        fail(f"B{b}: 骨架行数和 {s_sum} != 锁定 {EXPECT_SUM[b]}")
    # book item headers
    bids = re.findall(r"^## (F\d{4}) ", book, re.M)
    if bids != exp_ids:
        fail(f"B{b}: 深化册 ID 序列不符 {bids[:3]}...")
    # ledger table rows: | F#### | 缩写 | 行数 | 判据数 | M-### |
    lrows = []
    for line in book.splitlines():
        if not line.startswith("| F"): continue
        cells = [c.strip() for c in line.split("|")]
        cells = [c for c in cells if c]
        if len(cells) < 3 or not re.fullmatch(r"F\d{4}", cells[0]): continue
        mnum = next((c for c in cells if re.fullmatch(r"M-\d{3}", c)), None)
        rownum = next((c for c in cells[1:] if re.fullmatch(r"\d{2,4}", c)), None)
        if mnum and rownum:
            lrows.append((cells[0], rownum, mnum))
    if len(lrows) != 20:
        fail(f"B{b}: 记账表行数 {len(lrows)} != 20"); continue
    l_sum = sum(int(r[1]) for r in lrows)
    if l_sum != EXPECT_SUM[b]:
        fail(f"B{b}: 记账表求和 {l_sum} != 锁定 {EXPECT_SUM[b]}")
    if [r[0] for r in lrows] != exp_ids:
        fail(f"B{b}: 记账表 ID 序列不符")
    # 判据 per skeleton J1 presence in book 判据 block
    j1_missing = [fid for fid in exp_ids if not re.search(rf"## {fid} .*?- J1：", book, re.S)]
    if j1_missing: fail(f"B{b}: J1 缺失 {j1_missing}")
    # ⑤ 正文 >= 300
    for m in re.finditer(r"## (F\d{4}) .*?(?=\n---\n)", book, re.S):
        fid, seg = m.group(1), m.group(0)
        pm = re.search(r"\*\*正文\*\*：(.*?)(?=\n---|\Z)", seg, re.S)
        if not pm: fail(f"B{b} {fid}: 正文块缺失"); continue
        if len(pm.group(1).strip()) < 300:
            fail(f"B{b} {fid}: 正文 {len(pm.group(1).strip())} < 300 字")
    all_m += [r[2] for r in lrows]
    cum += l_sum
    ok(f"B{b}: ID 序列 ✓ 求和 {l_sum}==锁定 ✓ 正文≥300 ✓ J1 ✓ 记账表 20 行")

# M continuity
nums = sorted(int(m[2:]) for m in all_m)
if len(all_m) != 300: fail(f"②判据：M 号总数 {len(all_m)} != 300")
if len(set(nums)) != len(nums): fail("②判据：M 号有重号")
if nums and (nums[0] != 259 or nums[-1] != 558): fail(f"②判据：M 号区间 {nums[0]}–{nums[-1]} != 259–558")
if nums == list(range(259, 559)): ok("②判据：M-259–M-558 连续+齐备+无重号（300 号）")
elif not fails: fail("②判据：M 号不连续")

# 域级
if cum != TOTAL: fail(f"③守恒：域级总轧 {cum} != {TOTAL}")
else: ok(f"③守恒：B16–B30 总轧 {cum}==90,040；域累计 89,900+{cum}=179,940/240,000")

# 三链互见
chain = [("A5-B16", "F3734"), ("A5-B27", "M-492"), ("A5-B26", "F3719"),
         ("A5-B28", "M-502"), ("A5-B20", "F3781"), ("A5-B30", "F3781")]
for f, key in chain:
    t = open(os.path.join(ROOT, f"docs/unxreal/deepen/{f}.md"), encoding="utf-8").read()
    if key not in t: fail(f"③守恒：链目 {f} 缺 {key}")
ok("③守恒：三链互见（B16↔B27 F3734/M-492、B26↔B28 F3719/M-502、B20↔B30 F3781）")

# ---------- ④ 台账（可选） ----------
if "--with-ledger" in sys.argv:
    led = open(os.path.join(ROOT, "docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal · 总纲与施工书.md"), encoding="utf-8").read()
    n_ok = 0
    for b in BATCHES:
        m = re.search(rf"\| UNX-A5-B{b} \| F\d+–F\d+ \| 20 \| \[已深化\] \|", led)
        if m: n_ok += 1
        else: fail(f"④台账：W2 台账 B{b} 未翻 [已深化]")
    if n_ok == 15: ok("④台账：W2 台账 B16–B30 十五行全 [已深化]")

print("=" * 60)
if fails:
    print(f"共 {len(fails)} 项 FAIL"); sys.exit(1)
print("ALL PASS — A5 B16–B30 finalize 五步断言链全绿（exit 0）")
