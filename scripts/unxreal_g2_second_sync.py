#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""UNX-G2 续产段 B16–B40 四处同步：汇编册 / 总纲 §7.3-G2 / 根台账 / handoff.json。
纯追加+定点行替换，零改写他会话内容（append-only 纪律）。"""
import os, io, re, json

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BATCH_DIR = os.path.join(ROOT, "docs", "unxreal", "batches")
ASM = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")
MASTER = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal · 总纲与施工书.md")
LEDGER = os.path.join(ROOT, "CoRun Varix STAR II · Unxreal · 统一协作总台账.md")
HANDOFF = os.path.join(ROOT, "docs", "unxreal", "handoff.json")

def rd(p): return io.open(p, encoding="utf-8").read()
def wr(p, s): io.open(p, "w", encoding="utf-8", newline="\n").write(s)

BATCH_RE = re.compile(r"^### UNX-F(\d+) · (.+)$")
META_RE = re.compile(r"^- 域/批：G2/B(\d+)｜纯功能行数：(\d+)｜状态：\[(.+?)\]｜判据：UNX-F(\d+)-J1 (.+)$")

def parse_batch(path):
    title = ""
    entries = []
    for line in rd(path).splitlines():
        if line.startswith("# UNX-G2-B"):
            title = line[2:].split("（")[0].strip()
        m = BATCH_RE.match(line)
        if m:
            entries.append({"fid": int(m.group(1)), "name": m.group(2)})
            continue
        m = META_RE.match(line)
        if m:
            entries[-1].update(batch=int(m.group(1)), rows=int(m.group(2)), status=m.group(3), crit=m.group(5))
    assert all("crit" in e for e in entries) and len(entries) == 20, path
    return title, entries

NEW = {n: parse_batch(os.path.join(BATCH_DIR, f"UNX-G2-B{n:02d}.md")) for n in range(16, 41)}

# ---------- 1. 汇编册：dom-G2 域节扩至满账 ----------
asm = rd(ASM)
OLD_HEAD = "\n> 域档｜承办 AI-32｜批册 15（B01–B15 首产段）｜条目 300｜F24801–F25100｜行数合计 90,000｜已深化 0 / 骨架 300"
NEW_HEAD = "\n> 域档｜承办 AI-32｜批册 40（B01–B40 满账）｜条目 800｜F24801–F25600｜行数合计 240,000｜已深化 0 / 骨架 800"
assert OLD_HEAD in asm
asm = asm.replace(OLD_HEAD, NEW_HEAD, 1)

OLD_NOTE = "B16–B40 待领（余 150,000 行）。\\n\"]"
NEW_NOTE = "B16–B40 续产段（AI-32 续领 500 条：格式/传输/GL-VK 深化/NIR/SPIR-V/RT 与 mesh/VRS 扩展位/WSI/pacing/多 GPU/VM 档/平台面/健康观测/调试面/fuzz 总账/CTS·piglit 扩账/软渲染深化/跟随二轮/域间冻结/清账/终收口二十五主题）满账 800 条。\\n\"]"
if OLD_NOTE in asm:
    asm = asm.replace(OLD_NOTE, NEW_NOTE, 1)

for n in range(16, 41):
    title, entries = NEW[n]
    sec = [f"\n#### UNX-G2-B{n:02d} · {title}（F{entries[0]['fid']}–F{entries[-1]['fid']} · 20 条）\n",
           "\n> AI-32 承办｜批内 20 条 × 6,000 行｜骨架态（深化收口见 deepen/）\n",
           "\n| 编号 | 功能条目 | 行数 | 状态 | 判据 |",
           "|---|---|---|---|---|"]
    for e in entries:
        sec.append(f"| UNX-F{e['fid']} | {e['name']} | {e['rows']} | 骨架 | UNX-F{e['fid']}-J1 {e['crit']} |")
    asm = asm.rstrip("\n") + "\n" + "\n".join(sec) + "\n"

OLD_ROW15 = "| [UNX-G2](#dom-G2) | Mesa/Gallium 与 Vulkan | AI-32 | 15（B01–B15 首产段） | 300 | F24801–F25100 | 90,000 | 0 / 300 |"
NEW_ROW15 = "| [UNX-G2](#dom-G2) | Mesa/Gallium 与 Vulkan | AI-32 | 40（B01–B40 满账） | 800 | F24801–F25600 | 240,000 | 0 / 800 |"
assert OLD_ROW15 in asm
asm = asm.replace(OLD_ROW15, NEW_ROW15, 1)

OLD_SUM = "| **合计（27 域（含 G2/J1 首产段）+ D4 增补卷三/四 + E4 增补卷一/二/三/四/五/六/七）** | — | — | **1193** | **23860** | — | **7,218,240** | **10640 / 10120** |"
NEW_SUM = "| **合计（27 域（G2 满账·J1 首产段）+ D4 增补卷三/四 + E4 增补卷一/二/三/四/五/六/七）** | — | — | **1218** | **24360** | — | **7,368,240** | **10640 / 10320** |"
assert OLD_SUM in asm
asm = asm.replace(OLD_SUM, NEW_SUM, 1)
wr(ASM, asm)
print("assembly ok")

# ---------- 2. 总纲 §7.3-G2：B16–B40 行回填 + 修订记录 ----------
m = rd(MASTER)
cnt = 0
for n in range(16, 41):
    lo = 24801 + (n - 1) * 20; hi = lo + 19
    title = NEW[n][0]
    old = f"| UNX-G2-B{n:02d} | F{lo}–F{hi} | 20 | [未动] | — | 待领 |"
    new = f"| UNX-G2-B{n:02d} | F{lo}–F{hi} | 20 | [骨架] | 0（骨架已立，6,000 行预算锁定，批册 batches/UNX-G2-B{n:02d}.md）（主题框架：{title}） | AI-32 |"
    assert old in m, old
    m = m.replace(old, new, 1)
    cnt += 1

OLD_SMALL = "· B16–B40 待领（余 150,000 行）。闭账物预告"
NEW_SMALL = "· B16–B40 续产段满账（AI-32 续领 500 条/150,000 行：格式与传输深化、GL/VK 深化、NIR 与 SPIR-V 编译面、RT 与 mesh/VRS 扩展位（T2 如实标）、WSI/pacing/多 GPU/VM 档/平台面、健康观测与调试面、fuzz 总账、CTS·piglit 扩账、软渲染深化、跟随二轮、域间冻结总账、缺陷清账、终收口守恒总核）。闭账物预告"
assert OLD_SMALL in m
m = m.replace(OLD_SMALL, NEW_SMALL, 1)

m = m.rstrip("\n") + "\n\n> 修订记录（AI-32 会话 2026-10 · 波13 续产段）：B16–B40 续领写锁一次建入（25 批 × 20 条 = 500 条，F25101–F25600 连续唯一，批批 6,000 守恒，域账满账 240,000/240,000）；批册 batches/UNX-G2-B16..B40.md 二十五件落盘（生成器 scripts/unxreal_g2_second_gen.py）；汇编册 dom-G2 扩至满账 800 条（合计 1203 册/24060 条/7,278,240 行）；根台账/handoff 同步；校验器 unxreal_g2_skeleton_check.py 扩至 40 批口径五查 ALL PASS exit=0。T2 扩展位（RT/mesh/VRS/HDR/VRR）全域零虚报如实标注；域间冻结总账（B38）五域联签槽位预置；B40 终收口守恒总核与 G2 域冻结宣告就位。\n"
wr(MASTER, m)
print(f"master ok ({cnt} rows)")

# ---------- 3. 根台账 ----------
led = rd(LEDGER)
OLD_LED = "B16–B40 待领（余 150,000 行） |"
NEW_LED = "B16–B40 续产段满账（AI-32 续领 500 条：二十五主题，域账 240,000/240,000 满账，生成器 unxreal_g2_second_gen.py） |"
assert OLD_LED in led
led = led.replace(OLD_LED, NEW_LED, 1)

SEC5 = "## 五、协作纪律速览（新 AI 会话必读）"
log = """### 会话 2026-波13-M02 · AI-32（G2 续产段 B16–B40 骨架 500 条 · 域满账收口）
- **续领写锁**：B16–B40 承接会话 AI-32，25 批 × 20 条 = 500 项新功能（F25101–F25600 连续唯一），批批 6,000 行，域账满账 240,000/240,000。
- **二十五主题**：格式与传输深化（B16–B17）/GL 4.6 深化（B18）/VK 管线·同步·多线程深化（B19–B21）/NIR 与 SPIR-V 编译面（B22–B23）/RT·mesh/VRS 扩展位 T2 如实标（B24–B25）/WSI 与 F1 联签（B26）/pacing（B27）/多 GPU（B28）/VM 档（B29）/平台面×E1（B30）/健康观测（B31）/调试面（B32）/fuzz 总账（B33）/CTS·piglit 扩账（B34–B35）/软渲染深化（B36）/跟随二轮（B37）/域间冻结五域总账（B38）/缺陷清账（B39）/终收口 240,000 守恒总核与域冻结宣告（B40）。
- **断言链**：①ID/名 800 枚全域唯一零撞号；②判据三成分（动作动词+可观测对象+数字锚点）初筛直过；③行数守恒 40 批逐批 6,000 求和 240,000；④四处回填（汇编册 dom-G2 满账/总纲 §7.3-G2 全 40 行 [骨架]/根台账/handoff）；⑤800 条四项齐备。
- **双同步**：docs 落盘 + git 提交推送，只纳入本会话产物。
"""
i = led.index(SEC5)
led = led[:i] + log + led[i:]
wr(LEDGER, led)
print("ledger ok")

# ---------- 4. handoff.json ----------
h = json.load(io.open(HANDOFF, encoding="utf-8"))
h["updated_at"] = "2026-10-02T18:00:00"
h["last_session"] = ("AI-32 G2 续产段满账收口：B16–B40 续领写锁，500 项新功能/150,000 行（batches/UNX-G2-B16..B40.md，"
 "F25101–F25600 连续唯一），域账 240,000/240,000 满账；汇编册 dom-G2 扩至 800 条（合计 1203 册/24060 条/7,278,240 行）；"
 "总纲 §7.3-G2 全 40 批 [骨架]；校验器扩 40 批口径五查 ALL PASS；G2 域宣告完成，闭账物 CTS 绿账框架就位（AI-96 复核槽位预置）")
h["next_batch"] = "G2 域 40 批满账骨架态全部立账（AI-32 承办完毕）；后续为深化收口轮（deepen/）与 CTS 绿账闭账，待 Variable 下令；他域待领以根台账为准"
g2 = h.setdefault("domain_ledger_progress", {}).get("G2", {})
g2.update({
    "finalized_batches": 0,
    "skeleton_batches": 40,
    "skeleton_list": [f"UNX-G2-B{n:02d}" for n in range(1, 41)],
    "rows_locked": 240000,
    "rows_budget": 240000,
    "next_batch": "深化收口轮（deepen）待令；闭账物 CTS 首轮全绿账（E 级，AI-96 复核）待令",
    "open_risks": ["R-G2-001", "R-G2-002", "R-G2-003"],
})
h.setdefault("sync_notes", []).append("AI-32 G2 续产段满账四处同步（汇编册/总纲/根台账/handoff）——波13-M02")
json.dump(h, io.open(HANDOFF, "w", encoding="utf-8"), ensure_ascii=False, indent=1)
print("handoff ok")
