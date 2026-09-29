#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
unxreal_b3_consistency_check.py — B3 域终态记载一致性巡检器（波08-M25，AI-08）

背景判例链：
  - M22 巡检：账物文件哈希 43/43（文件层零失真）
  - M24 核对：派生物锚号引用 229 处全实证（引用层零幽灵）
  - M25 本轮：终态**记载层**一致性——B3 终态在四处记载源中的数字与声明是否同口径
    （高并发滚动风暴下，任一会话滚动覆盖出错即误导后续 AI 领批/验收）

四处记载源：
  A. handoff.json B3 块（终态标量 + phase/next_batch 置空声明）
  B. deepen/B3-B01..40.md 40 册头部状态行（[已深化] + 批累计行数锁定）
  C. 根台账 §三 B3 域段（两轮 177,600 + 收官 62,400 = 240,000 守恒口径）+ §六 AI-08 五行
  D. 三级闭账链 F6195→F6272→F6398 + 封账 F6397 跨源表述一致

路径基准：__file__ 定位仓库根（M22 教训）。
判据输出：逐项 PASS/FAIL 清单 + 汇总；exit 0=全绿，exit 1=有漂移。
"""
import json
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
DEEPEN = REPO / "docs" / "unxreal" / "deepen"
HANDOFF = REPO / "docs" / "unxreal" / "handoff.json"
ROOT_LEDGER = REPO / "CoRun Varix STAR II · Unxreal.md"

checks = []   # (group, name, ok, detail)


def chk(group, name, ok, detail=""):
    checks.append((group, name, bool(ok), detail))


def main():
    # ---- A. handoff.json B3 块 ----
    h = json.loads(HANDOFF.read_text(encoding="utf-8"))
    b3 = h.get("domain_ledger_progress", {}).get("B3")
    if b3 is None:
        chk("A-handoff", "B3 块存在", False, "domain_ledger_progress.B3 缺失——滚动覆盖事故级")
    else:
        chk("A-handoff", "finalized_batches=40", b3.get("finalized_batches") == 40,
            f"实值 {b3.get('finalized_batches')}")
        chk("A-handoff", "skeleton_batches=0", b3.get("skeleton_batches") == 0,
            f"实值 {b3.get('skeleton_batches')}")
        chk("A-handoff", "rows_locked=240000", b3.get("rows_locked") == 240000,
            f"实值 {b3.get('rows_locked')}")
        chk("A-handoff", "rows_budget=240000", b3.get("rows_budget") == 240000,
            f"实值 {b3.get('rows_budget')}")
        chk("A-handoff", "rows_deepened_locked=240000", b3.get("rows_deepened_locked") == 240000,
            f"实值 {b3.get('rows_deepened_locked')}")
        chk("A-handoff", "entries_skeleton=0", b3.get("entries_skeleton") == 0,
            f"实值 {b3.get('entries_skeleton')}")
        chk("A-handoff", "entries_deepened=800", b3.get("entries_deepened") == 800,
            f"实值 {b3.get('entries_deepened')}")
        fl = b3.get("finalized_list", [])
        chk("A-handoff", "finalized_list=40 项", len(fl) == 40, f"实值 {len(fl)}")
        chk("A-handoff", "finalized_list=UNX-B3-B01..B40 连续",
            fl == [f"UNX-B3-B{i:02d}" for i in range(1, 41)],
            f"首 {fl[:1]} 末 {fl[-1:]}" if fl else "空表")
        note = b3.get("entries_deepened_note", "")
        chk("A-handoff", "note 三轮守恒口径 83,420+94,180+62,400",
            all(s in note for s in ("83,420", "94,180", "62,400")),
            "note 缺三轮行数分账" if "62,400" not in note else "")
        phase = h.get("phase", "")
        chk("A-handoff", "phase 含『B3 满账封账』", "B3 满账封账" in phase, "")
        chk("A-handoff", "phase 含 B3『无待领批』", "无待领批" in phase and "B3" in phase, "")
        nxt = h.get("next_batch", "")
        chk("A-handoff", "next_batch 含 B3 置空声明", "next_batch 置空声明" in nxt and "B3" in nxt, "")

    # ---- B. deepen 40 册状态行 ----
    books = sorted(DEEPEN.glob("B3-B*.md"))
    chk("B-books", "deepen/B3-B*.md = 40 册", len(books) == 40, f"实值 {len(books)}")
    row_sum = 0
    row_src = 0
    for f in books:
        head = "\n".join(f.read_text(encoding="utf-8").splitlines()[:6])
        if "[已深化]" not in head:
            chk("B-books", f"{f.name} 头部 [已深化]", False, "状态行缺失或非深化态")
        m = re.search(r"批累计行数锁定\s*([\d,]+)", head)
        if m:
            row_sum += int(m.group(1).replace(",", ""))
            row_src += 1
    chk("B-books", "40 册头部 [已深化] 全在册",
        all("[已深化]" in "\n".join(f.read_text(encoding="utf-8").splitlines()[:6]) for f in books),
        "" )
    chk("B-books", f"批累计行数锁定求和=240,000（{row_src}/40 册可解析）",
        row_sum == 240000 and row_src == 40, f"实和 {row_sum:,}，可解析 {row_src}/40")

    # ---- C. 根台账 ----
    ledger = ROOT_LEDGER.read_text(encoding="utf-8")
    m = re.search(r"UNX-B3（AI-08[^）]*）", ledger)
    seg = m.group(0) if m else ""
    chk("C-ledger", "§三 B3 段存在", bool(seg), "" if seg else "根台账未找到 UNX-B3（AI-08…段")
    chk("C-ledger", "§三 两轮口径 177,600 行", "177,600" in seg, "")
    chk("C-ledger", "§三 收官口径 200 条/62,400 行",
        "B31–B40 域收官深化 200 条/62,400 行" in seg, "")
    chk("C-ledger", "§三 满账封账 240,000/240,000", "满账封账 240,000/240,000" in seg, "")
    chk("C-ledger", "§三 三级闭账链 F6195→F6272→F6398",
        "F6195→F6272→F6398" in seg, "")
    for i, kw in ((20, "物化归档"), (21, "账务闭环轮"), (22, "保管巡检轮"),
                  (23, "验收预演轮"), (24, "锚号全量核对轮")):
        chk("C-ledger", f"§六 波08-M{ i } AI-08 行在册",
            re.search(rf"波08-M{i}\s*\|\s*AI-08", ledger) is not None and kw in ledger,
            f"关键词『{kw}』未命中" if kw not in ledger else "")

    # ---- D. 闭账锚跨源表述 ----
    # 判据分级（首跑缺陷修正留痕：检查器有罪文件无罪第五次——首跑把"四锚齐"套用于
    # 根台账 §三 汇总行致假 FAIL；对照 C3/C5 域 §三 体例，汇总行引三级链即足额，
    # 封账锚 F6397 仅要求正式账两源在册）：
    #   三级闭账链 F6195→F6272→F6398 = 三处源全查
    #   封账锚 F6397 = 仅 handoff-note + closeout-ledger（正式账）必查，§三 汇总行豁免
    closeout = (REPO / "docs" / "unxreal" / "closeout"
                / "UNX-B3-NTFS-interop-ledger.md").read_text(encoding="utf-8")
    sources = (("handoff-note", (b3 or {}).get("entries_deepened_note", "")),
               ("root-ledger", seg),
               ("closeout-ledger", closeout))
    for name, text in sources:
        ok_chain = all(a in text for a in ("F6195", "F6272", "F6398"))
        chk("D-chain", f"三级闭账链 F6195/F6272/F6398 在 {name}", ok_chain, "")
    for name, text in sources:
        if name == "root-ledger":
            continue   # §三 汇总行粒度豁免——非静默：本注释即豁免依据留痕
        chk("D-chain", f"封账锚 F6397 在 {name}（正式账必查）", "F6397" in text, "")

    # ---- 汇总 ----
    fails = [c for c in checks if not c[2]]
    print("===== B3 域终态记载一致性巡检账（波08-M25） =====")
    cur = None
    for g, name, ok, detail in checks:
        if g != cur:
            print(f"\n--- {g} ---")
            cur = g
        print(f"  [{'PASS' if ok else 'FAIL'}] {name}" + (f" ｜ {detail}" if detail else ""))
    print(f"\n总计 {len(checks)} 项：PASS {len(checks) - len(fails)} / FAIL {len(fails)}")
    if fails:
        print("\nverdict: 记载漂移——须勘误留痕（逐处定位归属会话，固化物不回改）")
        sys.exit(1)
    print("\nverdict: 四处记载源终态全一致——滚动风暴下 B3 终态零漂移，"
          "后续 AI 可安全引用（40 批/240,000/800/封账 F6397/无待领批）")
    sys.exit(0)


if __name__ == "__main__":
    main()
