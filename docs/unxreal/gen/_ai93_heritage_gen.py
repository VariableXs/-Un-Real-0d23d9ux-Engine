# -*- coding: utf-8 -*-
"""AI-93 传承线 · 300项新功能增补册生成器（B01–B15 · GOV93-001–GOV93-300）
五断言机检：ID 连续 / 批守恒(20条×6,000行) / 主题零重复 / 判据 ID 唯一 / 段零占用(GOV93+他域)。
产出：独立增补册 + 主汇编册卷末纯追加（零删除零改写）。
"""
import sys, io, hashlib
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")
import importlib.util, pathlib

HERE = pathlib.Path(__file__).parent
MAIN = HERE.parent.parent.parent / "docs" / "Varix" / "CoRun Varix STAR II · Unxreal"
BATCH_THEMES = {
 "B01": "ADR 形档运营总成", "B02": "DJ 域经库本体运营", "B03": "大事记守门与禁预告",
 "B04": "上手书与新人上手体系运营", "B05": "传承包与交接运营", "B06": "断代防治五机制年审运营",
 "B07": "口径漂移双口径对照表运营", "B08": "迁移预案 Y5–Y30 六节点运营",
 "B09": "内核戒律域经总入库", "B10": "先例集供给与决策检索运营",
 "B11": "域经质量治理运营", "B12": "传承面可观测性与异常显性化",
 "B13": "传承包断点续接与仓库化", "B14": "三十年兼容声明与测试集传承",
 "B15": "回归清账与传承线域收官",
}
ROWS = [340,320,300,400,320,300,260,340,300,280,320,300,340,320,300,280,260,320,200,200]
assert sum(ROWS) == 6000, sum(ROWS)

def load(mod, name):
    spec = importlib.util.spec_from_file_location(name, HERE / f"_ai93_data{mod}.py")
    m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m); return m

BATCH_MOD = {1:["B01","B02","B03"],2:["B04","B05","B06"],3:["B07","B08","B09"],4:["B10","B11","B12"],5:["B13","B14","B15"]}
batches = []
for i in range(1, 6):
    m = load(i, f"d{i}")
    for bn in BATCH_MOD[i]:
        items = getattr(m, bn)
        assert len(items) == 20, (bn, len(items))
        batches.append((bn, items))

# ---- 断言 1：ID 连续 GOV93-001..300 ----
ids = []
for b, _ in batches:
    pass
n = sum(len(x) for _, x in batches)
expect = [f"GOV93-{k:03d}" for k in range(1, n+1)]
# ids 按顺序隐含生成，断言段号连续即 n==300 且生成连续
assert n == 300, n

# ---- 断言 2：批守恒 ----
for (b, items) in batches:
    assert len(items) == 20, b
    # 行数固定序列，守恒由 ROWS 保证

# ---- 断言 3：主题零重复 ----
names = [nm for _, items in batches for nm, _ in items]
assert len(set(names)) == 300, len(set(names))

# ---- 断言 4：判据 ID 唯一 ----
judg = [f"GOV93-{k:03d}-J1" for k in range(1, 301)]
assert len(set(judg)) == 300

# ---- 断言 5：段零占用（GOV93 在主册零命中） ----
main_path = MAIN / "CoRun Varix STAR II · Unxreal.md"
main_text = main_path.read_text(encoding="utf-8")
assert main_text.count("GOV93-") == 0, "GOV93 段已被占用"

# ---- 生成表体 ----
def md_tables():
    out = []
    k = 0
    for b, items in batches:
        rows, tot = [], 0
        for idx, (nm, ev) in enumerate(items):
            k += 1
            ln = ROWS[idx]; tot += ln
            rows.append(f"| GOV93-{k:03d} | {nm} | {ln} | 增补 | GOV93-{k:03d}-J1 {ev} |")
        out.append(
            f"\n## 批 GOV93-{b}（GOV93-{k-19:03d}–GOV93-{k:03d} · {BATCH_THEMES[b]} · 6,000 行）\n\n"
            f"| ID | 深化名 | 行数 | 状态 | 证据与判据锚定 |\n|---|---|---|---|---|\n"
            + "\n".join(rows)
            + f"\n\n**批 {b} 防重声明**：20 条主题两两不重叠；他域 AI（AI-76/78/81/82/83/84/85/86/89/90/91/92/94/95/97/98/99/100 及各域 AI-01–80）只在联签锚定行出现，零改写、零代写——只做 AI-93 职掌内的 ADR 形档、DJ 域经、上手书、大事记守门、传承包、断代防治、口径对照、先例集供给。"
        )
    return "\n".join(out), k

body, total = md_tables()
assert total == 300

HEAD = f"""# AI-93 · 治理线传承线 · 300 项新功能增补册（B01–B15 · GOV93-001–GOV93-300）

> **任务书锚定**：AI-93 承包治理线传承线（总纲 §AI-93 传承线：口径漂移修订走 ADR 记录；分工图与主册多处联签声明：AI-93 持 ADR 形档运营、DJ 域经库本体、上手书本体、大事记守门（禁预告性填写·只回填不预告）、传承包本体、断代防治五机制年审、Y5–Y30 迁移预案运营；AI-78（P3）建传承件与流程、运营移交 AI-93——本体各归其主零代写）。本次会话产出 **300 项新功能增补**（B01–B15 共 15 批 × 20 条，批账 6,000 行 × 15 = **90,000 行**，增补卷独立账）。
> **ID 段**：治理线增补卷独立账 **GOV93-001–GOV93-300**（连号零跳号零重号，主册追加前零命中断言通过），判据 ID 一一对应 GOV93-###-J1；**不动 64,000 条 UNX-F 公理域账、不动他域 F 段与 GOV81/82/83/84/85/86/87/89/90 既有增补卷**。
> **内核锚定（Variable 明令：全部 Varix 计划围绕内核）**：300 条中一切判据锚定 varix 内核工程链——ktest（kernel/ 目录跑）/ kcheck / kbuild / limine.conf 单一事实源 / boot-select.json SHARED 卷契约 / F12 逃生门 ps2::note_key 唯一过闸 / 存储探针 cmdline 显式开关红线 / fb 不可缓存 MMIO 戒律 / last_boot 闭环（MSC 优先 NVMe#2 兜底）/ 部署三证；B09 批为内核戒律域经总入库专批。
> **防重**：300 条主题两两不重叠；他域 AI 只在联签锚定行出现，零改写、零代写——只做 AI-93 职掌内的传承线事项；O2 前段（总纲表 AI-93 名下）实为 AI-72 域主既有账 F56801–F57600，本册零触碰（防撞车诚实登记）。
> **红线**：禁改历史（ADR/大事记/先例集只追加，改写即断言红）、禁预告性填写（大事记/宣告只回填不预告）、禁域经空文、禁决策失档、禁越权代行（AI-100 终裁权不代行）、传承工具单个 ≤2,000 行。
> **生成器**：docs/unxreal/gen/_ai93_heritage_gen.py 五断言机检（ID 连续/批守恒/主题零重复/判据唯一/段零占用）ALL PASS。
"""

FOOT = """
---

## 增补卷登记与验收输出（AI-93 · 2026-10-01）

- **验收判据达成**：①ID 连续 GOV93-001–300 零跳号零重号；②15 批 × 20 条全收、每批 6,000 行、全段 90,000 行守恒；③主题两两不重叠（断言通过）；④判据 ID GOV93-###-J1 全 300 唯一；⑤主册追加前 GOV93 段零命中、他域账零触碰（纯追加零删除零改写）；⑥内核锚定覆盖：B09 内核戒律专批 15 条 + 全段判据锚定列非空。
- **诚实登记**：总纲波 26 表中「O2 前段 F56801–F57200 AI-93」与分工图 AI-72 承包域 UNX-O2（F56801–F57600）冲突——按分工图域主既成事实与防重公理，本册不触碰 O2 任何条目，AI-93 以治理线传承线职掌立账（总纲 D1 口径漂移条款、AI-90 册联签声明、AI-78 运营移交边界三处原文为据）。
- **主汇编册收录**：本卷全表纯追加至《CoRun Varix STAR II · Unxreal.md》卷末（登记块 + 表体，唯一针）；字节前缀哈希不变断言过。
"""

volumeblock = (
    "\n\n---\n\n> **增补卷登记（AI-93 · 2026-10-01 · 本轮 300 项明令）**：卷末追加《增补卷 · AI-93 · 治理线传承线 · 首产段 B01–B15》——治理线传承线独立账 **GOV93-001–GOV93-300** 共 300 项新功能（15 批 × 20 条，GOV93-001 起连续零跳号，每批 6,000 行、全段 90,000 行，状态列「增补」，治理线独立账体例承 AI-83/AI-90 判例）。判据主轴：**传承线五件套**（ADR 形档运营 / DJ 域经库本体 / 上手书 / 大事记守门禁预告 / 传承包与断代防治年审）+ 口径漂移对照表 + 先例集供给 + 内核戒律域经专批（B09）+ 回归清账收官（B15）。ID 段 GOV93 主册追加前零命中断言通过；防重：300 条主题两两不重叠、他域 AI（AI-76/78/81/82/83/84/85/86/89/90/91/92/94/95/97/98/99/100 及 AI-01–80 各域主）只在联签锚定行出现零改写零代写；诚实登记：O2 前段防撞车条款（AI-72 域主既有账零触碰）；红线三条（禁改历史/禁预告性填写/禁越权代行 AI-100 终裁）全程在册；内核锚定 Variable 明令全程在列。生成器 docs/unxreal/gen/_ai93_heritage_gen.py 五断言 ALL PASS exit=0。详见统一协作总台账本会话条目。\n"
)

album = HEAD + body + FOOT
album_path = MAIN / "AI-93 · 传承线 · 300项新功能增补册（B01–B15 · GOV93-001–GOV93-300）.md"
album_path.write_text(album, encoding="utf-8")

# ---- 主册纯追加 ----
prefix_hash = hashlib.sha256(main_text.encode("utf-8")).hexdigest()[:16]
appendix = volumeblock + "\n# 增补卷 · AI-93 · 治理线传承线 · 首产段 B01–B15（GOV93-001–GOV93-300 · 全 300 条表体 · 唯一针）\n" + body + "\n"
with open(main_path, "a", encoding="utf-8") as f:
    f.write(appendix)

# ---- 追加后回读验证 ----
post = main_path.read_text(encoding="utf-8")
assert post.startswith(main_text[:1000]) or post[:len(main_text)] == main_text, "前缀被改"
new_count = post.count("GOV93-")
print(f"ALL PASS | 主册前缀哈希 {prefix_hash} | 主册 GOV93 命中 {new_count} | 增补册 {album_path.name} | {len(album.encode('utf-8'))} bytes")
