# -*- coding: utf-8 -*-
"""UNX-F5 域账机械校验器（AI-30 · 40 批满账口径）五查：
①批册 40 册齐装、逐册 20 条 ②ID F23201–F24000 连续唯一 ③行数守恒批批 6,000/总 240,000
④判据 J1 齐备 800/800 ⑤汇编册域节与批册一致（抽样 60）+ 防重（与 E 增补卷判据文本零重复）
"""
import re, sys, glob, random

BATCH_DIR = "docs/unxreal/batches"
MD = "docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md"
N_BATCH, LO, HI = 40, 23201, 24000
fails = []

def batch_of(fid):
    return f"B{(fid-23200+19)//20:02d}"

ROW = lambda bid: (r"^### UNX-F(\d+) · (.+)$\n- 域/批：F5/"+bid+r"｜纯功能行数：(\d+)｜状态：\[骨架\]｜判据：UNX-F\1-J1 (.+)$")

# ①②③④ 批册
ids, total = [], 0
files = sorted(glob.glob(f"{BATCH_DIR}/UNX-F5-B*.md"))
if len(files) != N_BATCH:
    fails.append(f"批册数 {len(files)} != {N_BATCH}")
cache = {}
for fp in files:
    txt = open(fp, encoding="utf-8").read()
    bid = re.search(r"UNX-F5-(B\d{2})", fp).group(1)
    rows = re.findall(ROW(bid), txt, re.M)
    cache[bid] = rows
    if len(rows) != 20:
        fails.append(f"{bid} 条数 {len(rows)} != 20")
    s = sum(int(r[2]) for r in rows)
    if s != 6000:
        fails.append(f"{bid} 行数和 {s} != 6000")
    total += s
    for r in rows:
        ids.append(int(r[0]))
        if not r[3].strip():
            fails.append(f"F{r[0]} 判据为空")
if ids != list(range(LO, HI+1)):
    fails.append(f"ID 区间非 F{LO}–F{HI} 连续唯一（实得 {len(ids)} 条）")
if total != (HI-LO+1)*300:
    fails.append(f"总轧 {total} != {(HI-LO+1)*300}")

# ⑤ 汇编册域节一致（双体例合并：首产段表格式 + 第二产段列表式；抽样 42）
md = open(MD, encoding="utf-8").read()
tbl_rows = [(int(a), b, c, d) for a,b,c,d in re.findall(r"^\| UNX-F(\d+) \| (.+?) \| (\d+) \| 骨架 \| UNX-F\1-J1 (.+?) \|$", md, re.M)]
lst_rows = [(int(a), b, c, d) for a,b,c,d in re.findall(r"^### UNX-F(\d+) · (.+)$\n- 域/批：F5/B\d+｜纯功能行数：(\d+)｜状态：\[骨架\]｜判据：UNX-F\1-J1 (.+)$", md, re.M)]
merged = {}
for fid,t,c,j in tbl_rows + lst_rows:
    if LO <= fid <= HI:
        if fid in merged and merged[fid] != (t,c,j):
            fails.append(f"F{fid} 汇编双体例不一致"); break
        merged[fid] = (t,c,j)
if sorted(merged) != list(range(LO, HI+1)):
    fails.append(f"汇编册域节 ID {len(merged)} != {HI-LO+1} 连续")
random.seed(30)
sample = sorted(set([LO, HI] + random.sample(range(LO, HI+1), 58)))
for fid in sample:
    if fid not in merged:
        fails.append(f"F{fid} 汇编缺失"); continue
    bid = batch_of(fid)
    brow = [r for r in cache.get(bid, []) if int(r[0]) == fid]
    if not brow:
        fails.append(f"F{fid} 批册缺失"); continue
    brow = brow[0]
    if merged[fid][0] != brow[1] or merged[fid][1] != brow[2] or merged[fid][2] != brow[3]:
        fails.append(f"F{fid} 汇编与批册不一致")

# 防重：与 E 增补卷判据文本零重复（整行判据级）
e_crit = set(re.findall(r"UNX-F5-E\d{3}-J1 (.+?) \|", md, re.M))
b_crit = [merged[f][2] for f in range(LO, HI+1) if f in merged]
dup = [c for c in b_crit if c in e_crit]
if dup:
    fails.append(f"与 E 增补卷判据重复 {len(dup)} 条")
# 域内跨批判据整行零重复——治理固定项（ktest 断言集/批级守恒预核/跨批接口冻结注记/批收口聚合/批防重终扫）
# 依 §7.6 批级固定项体例跨批同文（AI-28 F3-B16 判例同款），豁免；实质条目必须零重复
GOV = re.compile(r"^(ktest 断言集|批级守恒预核与防重扫描|跨批接口冻结注记|批收口与体验日志聚合|批防重终扫与红线声明核验)")
seen, d2 = set(), []
for fp in files:
    bid = re.search(r"UNX-F5-(B\d{2})", fp).group(1)
    for r in cache.get(bid, []):
        if GOV.match(r[1]):
            continue
        c = r[3]
        if c in seen: d2.append((r[0], c))
        seen.add(c)
if d2:
    fails.append(f"域内实质判据跨批重复 {len(d2)} 条: " + "; ".join(f"F{i}:{c[:30]}" for i,c in d2[:5]))

if fails:
    print("FAIL", len(fails))
    [print(" -", f) for f in fails[:20]]
    sys.exit(1)
print(f"unxreal_f5_skeleton_check: 五查 ALL PASS exit 0（{N_BATCH} 册/{HI-LO+1} 条/{(HI-LO+1)*300:,} 行/判据 {HI-LO+1}/汇编一致抽样 {len(sample)}/防重零重复含域内跨批）")
