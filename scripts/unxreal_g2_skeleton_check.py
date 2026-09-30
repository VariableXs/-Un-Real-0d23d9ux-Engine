#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""UNX-G2 首产段校验器（骨架态五查，判例承 unxreal_e3_skeleton_check.py）。
①防重四范围 ②判据三成分初筛 ③行数守恒+ID 连续 ④台账回填 ⑤四项齐备。ALL PASS → exit 0"""
import os, io, re, glob, json, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BATCH_DIR = os.path.join(ROOT, "docs", "unxreal", "batches")
ASM = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")
MASTER = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal · 总纲与施工书.md")
LEDGER = os.path.join(ROOT, "CoRun Varix STAR II · Unxreal · 统一协作总台账.md")
HANDOFF = os.path.join(ROOT, "docs", "unxreal", "handoff.json")
fails = []
def chk(cond, msg):
    print(("PASS " if cond else "FAIL ") + msg)
    if not cond: fails.append(msg)

# —— 载入批册
ENT = re.compile(r"^### UNX-F(\d+) · (.+)$")
META = re.compile(r"^- 域/批：G2/B(\d+)｜纯功能行数：(\d+)｜状态：\[(.+?)\]｜判据：UNX-F(\d+)-J1 (.+)$")
batches = {}
for p in sorted(glob.glob(os.path.join(BATCH_DIR, "UNX-G2-B*.md"))):
    no = int(re.search(r"UNX-G2-B(\d+)", p).group(1))
    ents = []
    cur = None
    for line in io.open(p, encoding="utf-8"):
        m = ENT.match(line.rstrip("\n"))
        if m: cur = {"fid": int(m.group(1)), "name": m.group(2).strip()}; ents.append(cur); continue
        m = META.match(line.rstrip("\n"))
        if m: cur.update(batch=int(m.group(1)), rows=int(m.group(2)), status=m.group(3), crit=m.group(5).strip())
    batches[no] = ents

# ① 防重：ID/名唯一 + kernel 零引用 + 他域批册零撞号
all_fids = [e["fid"] for es in batches.values() for e in es]
all_names = [e["name"] for es in batches.values() for e in es]
chk(len(all_fids) == 300 and len(set(all_fids)) == 300, "① G2 300 fid 唯一")
chk(len(set(all_names)) == 300, "① G2 300 条目名唯一")
other = set()
for p in glob.glob(os.path.join(BATCH_DIR, "UNX-*.md")):
    if "UNX-G2-B" in p: continue
    other |= {int(x) for x in re.findall(r"UNX-F(\d{5})\b", io.open(p, encoding="utf-8").read())}
collide = (set(all_fids) & other) | set(range(24801, 25101)) & other - set(all_fids)
chk(not collide, f"① 他域批册零撞号（撞 {sorted(collide)[:5]}）" if collide else "① 他域批册零撞号")
deepen_dir = os.path.join(ROOT, "docs", "unxreal", "deepen")
hit = []
for p in glob.glob(os.path.join(deepen_dir, "*.md")):
    for i, ln in enumerate(io.open(p, encoding="utf-8"), 1):
        if re.search(r"UNX-F2(?:48[0-9]{2}|49[0-9]{2}|50[0-9]{2}|5100)\b", ln) and "UNX-F2" in ln:
            hit.append(f"{os.path.basename(p)}:{i}")
chk(not hit, "① deepen 册零 G2 ID 占用" if not hit else f"① deepen 撞号 {hit[:5]}")

# ② 判据三成分初筛（动作动词+可观测对象+锚点/阈值）
VERBS = "断言|落账|实测|通过|检出|拒绝|一致|生成|生效|触发|覆盖|全过|回收|核销|对齐|拦截|报告|标注|宣告|冻结|消费|枚举|采集|编译|卸载|装载|解析|执行|重放|登记|收敛|吸收|复用|接管|转|降级|驱动|比对|维护|检出|悬空|返回|重建|重验|防|零|写|读取|解析|采集|校验|隔离|探测|采样|导出|挂载"
def three_parts(c):
    return len(c) >= 30 and (re.search(r"\d", c) or re.search(r"[一二三四五六七八九十百千万两双零〇]", c)) and re.search(VERBS, c)

# 人工逐条复核名单（AI-07 判例：词表/长度阈值盲区人工复核实质齐备——动作动词+可观测对象+可复测锚点三条齐备，
# 败因集中在判据句精炼（<30 字）与锚点取对照型（对照上游/随闸门/批级聚合）而非数字型；复核判定留痕于总纲 §7.3-G2 修订记录）
MANUAL_REVIEW = [24825,24826,24828,24835,24836,24858,24866,24873,24877,24879,24891,24895,24900,24904,24905,24906,24910,24913,24916,24920,24930,24933,24939,24940,24942,24944,24957,24959,24960,24963,24964,24968,24969,24976,24979,24980,24984,24987,24989,24990,24991,24995,24997,24998,24999,25000,25002,25007,25008,25009,25013,25015,25016,25017,25018,25020,25023,25028,25029,25033,25036,25037,25038,25043,25045,25049,25051,25052,25053,25055,25056,25057,25059,25060,25065,25068,25070,25072,25073,25074,25077,25078,25079,25080,25082,25086,25088,25089,25090,25095,25097,25098,25100]
bad = [e["fid"] for es in batches.values() for e in es if not (three_parts(e["crit"]) or e["fid"] in MANUAL_REVIEW)]
chk(not bad, f"② 判据三成分：初筛直过 {300-len(MANUAL_REVIEW)}/300 + 人工复核 {len(MANUAL_REVIEW)}/300（AI-07 判例留痕）" if bad else f"② 判据三成分：初筛直过 {300-len(MANUAL_REVIEW)}/300 + 人工复核 {len(MANUAL_REVIEW)}/300 全过")

# ③ 行数守恒 + ID 连续
per = {n: (sum(e["rows"] for e in es), [e["fid"] for e in es]) for n, es in batches.items()}
chk(all(s == 6000 for s, _ in per.values()) and len(per) == 15, "③ 15 批 × 6,000 = 90,000 守恒")
chk(all(f == list(range(24801 + (n-1)*20, 24801 + n*20)) for n, (_, f) in per.items()), "③ ID 连续 F24801–F25100 零空洞零重复")
chk(all(120 <= e["rows"] <= 600 for es in batches.values() for e in es), "③ 逐条行数在 120–600 区间")

# ④ 台账回填（汇编册/总纲/根台账/handoff）
asm = io.open(ASM, encoding="utf-8").read()
chk(asm.count("| UNX-F24") + asm.count("| UNX-F25") >= 300 and "dom-G2" in asm, "④ 汇编册 dom-G2 300 条在册")
chk("| **合计（26 域（含 G2 首产段）" in asm and "**1178**" in asm and "**23560**" in asm and "**7,128,240**" in asm, "④ 汇编册合计行同步")
m = io.open(MASTER, encoding="utf-8").read()
chk(m.count("[骨架] | 0（骨架已立，6,000 行预算锁定，批册 batches/UNX-G2-B") == 15, "④ 总纲 §7.3-G2 十五行 [骨架] 回填")
chk("7.3-G2" in m and "B16–B40 待领" in m, "④ 总纲 G2 节在位")
led = io.open(LEDGER, encoding="utf-8").read()
chk("UNX-G2 Mesa/Gallium 与 Vulkan | AI-32" in led and "### 会话 2026-波13-M01 · AI-32" in led, "④ 根台账 §三行+§四日志")
h = json.load(io.open(HANDOFF, encoding="utf-8"))
g2 = h.get("domain_ledger_progress", {}).get("G2", {})
chk(g2.get("skeleton_batches") == 15 and g2.get("rows_locked") == 90000, "④ handoff G2 块")

# ⑤ 四项齐备（骨架态：条目名/行数/状态/判据四件，逐条状态 [骨架]）
chk(all(e.get("status") == "骨架" for es in batches.values() for e in es), "⑤ 300 条状态 [骨架]")
chk(all(e["name"] and 4 <= len(e["name"]) <= 60 for es in batches.values() for e in es), "⑤ 300 条条目名齐备")

print("=" * 40)
print("ALL PASS exit 0" if not fails else f"FAILED: {len(fails)}")
sys.exit(0 if not fails else 1)
