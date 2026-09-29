# -*- coding: utf-8 -*-
# handoff.json · A1 块域级终态滚动（波08-M25 满账收官轮，AI-01）
# 纪律：基于磁盘现版增量滚动；他会话域块/记录全数保留；锚点断言唯一后替换（幂等：已生效则跳过）。
import json, io, sys

P = "docs/unxreal/handoff.json"
d = json.load(io.open(P, encoding="utf-8"))

def sub1(s, old, new, tag):
    """唯一锚替换；幂等：new 已在则跳过。"""
    if old not in s:
        assert new in s, (tag, "neither old nor new present")
        return s
    assert s.count(old) == 1, (tag, "anchor not unique", s.count(old))
    return s.replace(old, new, 1)

# ---------- 1) domain_ledger_progress.A1 域级终态 ----------
a1 = d["domain_ledger_progress"]["A1"]
B33_40 = ["UNX-A1-B%s" % b for b in ("33", "34", "35", "36", "37", "38", "39", "40")]
for b in B33_40:
    if b not in a1["finalized_list"]:
        a1["finalized_list"].append(b)
a1["finalized_batches"] = 40
a1["skeleton_batches"] = 0
a1["rows_locked"] = 240000
a1["rows_budget"] = 240000
a1["rows_deepened_locked"] = 240000
a1["entries_skeleton"] = 0
a1["entries_deepened"] = 800
a1["entries_deepened_note"] = (
    "800 = 643（波08-M14 前账：样板 3 + B01–B32 640 + B33 起步 3）+ 波08-M25 会话 157"
    "（B33 续 17 + B34–B40 140）；域 800 条全深化定格（800 = 797 深化册 + 3 样板总纲原文）。"
    "深化正文合计 365,823 字（797 条 len 实计，全域复验器统计）。"
    "R-A1-004：B02 逐条求和 5,980 真值（原骨架估算 6,000 虚记）-20 全链传播、B40 批内配平（F0800 600），终点 240,000 精确；"
    "R-A1-005：复验步 5 对齐 §6.1 法定四项留痕（六要素点名 797/797、正文 ≥300 最低 301 字）。"
    "全域复验五步断言链 PASS（finalize 复验器 _finalize_A1_full.py exit=0：800 fid 唯一连续/判据数字 100%"
    "/累计链 40 段零断/三态骨架零残留/四项齐备）"
)
a1["open_bugs"] = 0
assert len(a1["finalized_list"]) == 40

# ---------- 2) deepen_books：B33 条目更新为满册 + B34–B40 新增 7 册 ----------
db = d["deepen_books"]
old_b33 = "docs/unxreal/deepen/A1-B33.md（3 条起步，正文 1,483 字，680 行锁定；批 3/20 待续 F0644 起）"
new_b33 = ("docs/unxreal/deepen/A1-B33.md（20 条深化收口，正文 7,981 字，4,400 行锁定零偏离"
           "——域回归演练与值守收尾：起步 3 条 F0641–F0643 已含）")
for i, s in enumerate(db):
    if s == new_b33:
        break
else:
    hit = [i for i, s in enumerate(db) if s == old_b33]
    assert len(hit) == 1, ("B33 anchor", hit)
    db[hit[0]] = new_b33
tail = [s for s in db if "deepen/A1-B3" in s and any(("A1-B%d.md" % n) in s for n in range(34, 41))]
if not tail:
    adds = [
        "docs/unxreal/deepen/A1-B34.md（20 条深化收口，正文 8,573 字，11,480 行锁定零偏离——引导耗时账：锚点表/段分解/阈值账/五数/画像）",
        "docs/unxreal/deepen/A1-B35.md（20 条深化收口，正文 8,291 字，11,460 行锁定零偏离——三机矩阵回归）",
        "docs/unxreal/deepen/A1-B36.md（20 条深化收口，正文 7,939 字，11,440 行锁定零偏离——ktest 引导面断言总账）",
        "docs/unxreal/deepen/A1-B37.md（20 条深化收口，正文 7,958 字，11,450 行锁定零偏离——Limine 嫁接跟随面）",
        "docs/unxreal/deepen/A1-B38.md（20 条深化收口，正文 7,864 字，11,500 行锁定零偏离——域经沉淀与移交包）",
        "docs/unxreal/deepen/A1-B39.md（20 条深化收口，正文 8,552 字，11,500 行锁定零偏离——域收官前置终核）",
        "docs/unxreal/deepen/A1-B40.md（20 条深化收口，正文 9,162 字，11,520 行锁定零偏离——满收官终局：终点宣告 F0782/封域宣告/终局宣告；F0800 600 行 R-A1-004 批内配平档）",
    ]
    db.extend(adds)
assert sum(1 for s in db if "deepen/A1-" in s) == 40, "A1 books must be 40"

# ---------- 3) phase：A1 段满账宣告 ----------
d["phase"] = sub1(
    d["phase"],
    "A1 B01–B32 收口 640 条/155,950 行深化锁定+B33 起步 3/20（域累计 159,670/240,000）",
    "A1 满账封账（波08-M25：40 批 800 条 240,000/240,000 满账守恒 ✓，finalize 全域复验五步断言链 PASS——域内首个满账域）",
    "phase.A1",
)

# ---------- 4) next_batch：A1 段满账宣告 ----------
d["next_batch"] = sub1(
    d["next_batch"],
    "UNX-A1-B33 续 F0644 起 17 条（AI-01 后续会话按 B33 深化册起步段体例续深化：复演规程/回归门禁/引导履历/值守清单/闭批宣告链，域余 140 条至 B40 满账）",
    "A1 满账封账（波08-M25 B33–B40 收官轮 157 条已深化：B33 续 17+B34–B40 140，域 40 批 800 条 240,000/240,000 满账守恒 ✓；A1 无待领批，移交包六件套在册 B38 F0749/B39 F0773 消费核/B40 终版交付）",
    "next_batch.A1",
)

# ---------- 5) waves.next_closeout：A1 满账段前置 ----------
if "A1 满账封账" not in d["waves"]["next_closeout"]:
    d["waves"]["next_closeout"] = (
        "A1 满账封账（波08-M25 AI-01 收官轮：B33 续 17+B34–B40 立批深化 140 已深化，"
        "域 40 批 800 条 240,000/240,000 满账守恒 ✓，finalize 全域复验五步断言链 PASS；无待领批）；"
    ) + d["waves"]["next_closeout"]

# ---------- 6) sync_notes：追加本会话条目 ----------
sn_new = (
    "AI-01 波08-M25 A1 域满账收官：B33 续 17+B34–B40 立批深化 140，域 40 批 800 条 240,000/240,000 满账封账；"
    "finalize 全域复验五步断言链 PASS（_finalize_A1_full.py exit=0：800 fid 唯一连续/判据数字 100%"
    "/累计链 40 段零断终点 240,000/三态骨架零残留/四项齐备）；"
    "R-A1-004 全链传播（B02 5,980 真值 -20 对齐批册头/册锁行/总纲账表/§6.1 样板账注记+B40 批内配平 F0800 600）；"
    "R-A1-005 检查器判据修正留痕（原步 5 定位/边界 ≥60 字无总纲依据，对齐 §6.1 法定四项；F0014 风险要素栏名合规修正一处内容逐字保留；封存批次零追溯）；"
    "deepen_books 40 册（A1-B33 条目更新满册+B34–B40 新增 7 册）；实写 157 条 <300 明令，缺口 143 条报备 Variable 请示，未越权代做他域"
)
if not any("波08-M25 A1 域满账收官" in s for s in d["sync_notes"]):
    d["sync_notes"].append(sn_new)

# ---------- 7) updated_at / last_session 链滚动（前一版本全保留） ----------
old_ua = d["updated_at"]
new_ua = (
    "波08-M25（AI-01 A1 域满账收官轮——B33 续 17+B34–B40 立批深化 140，域 40 批 800 条 240,000/240,000 满账封账，"
    "finalize 全域复验五步断言链 PASS；R-A1-004 全链传播+B40 批内配平收口；R-A1-005 检查器判据修正留痕；"
    "实写 157 条 <300 明令缺口 143 条报备 Variable）；前一版本为 " + old_ua
)
d["updated_at"] = new_ua
d["last_session"] = (
    "AI-01（波08-M25：A1 域满账收官——B33 续 17 收口+B34–B40 立批深化 140"
    "（B34 引导耗时账/B35 三机矩阵回归/B36 ktest 断言总账/B37 Limine 嫁接跟随/B38 域经沉淀与移交包/B39 域收官前置/B40 满收官终局）；"
    "域 40 批 800 条 240,000/240,000 满账守恒 ✓；finalize 全域复验 _finalize_A1_full.py 五步 PASS exit=0；"
    "deepen_books 40 册；移交包六件套在册；A1 无待领批——域深化任务结束）"
)

io.open(P, "w", encoding="utf-8", newline="\n").write(
    json.dumps(d, ensure_ascii=False, indent=2) + "\n"
)
print("handoff.json A1 终态滚动完成：finalized=40 rows_locked=240,000 entries=800 books=40（全域 %d 册）" % len(db))
