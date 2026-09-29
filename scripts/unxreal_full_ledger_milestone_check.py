#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
unxreal_full_ledger_milestone_check.py — 满账域里程碑对账器（波08-M26，AI-08）

背景：全域满账域增至 5 个（A1 引导/B3 NTFS/C3 演习执行/C5 集成联测/D1 NT API 语义面），
各域 40 批 800 条 240,000 行满账封账——合计 1,200,000 行里程碑。

本器做**只读**交叉对账（零改动他域记载，不越权纪律）：
  ① handoff.json 五域块终态标量（8 项 × 5 域）
  ② phase 五域满账/收官声明
  ③ 根台账 §三 五域段 240,000/240,000 满账口径
  ④ 五域闭账锚本域批册在册（F0800/F6397/F10400/F12000/F12800）
  ⑤ 里程碑算术 Σ=1,200,000

路径基准：__file__ 定位仓库根（M22 教训）。
判据输出：逐项 PASS/FAIL + 汇总；exit 0=对账全绿，exit 1=有漂移。
"""
import json
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
HANDOFF = REPO / "docs" / "unxreal" / "handoff.json"
ROOT_LEDGER = REPO / "CoRun Varix STAR II · Unxreal.md"
DEEPEN = REPO / "docs" / "unxreal" / "deepen"

# 五满账域：域号 / 闭账锚 / phase 关键词 / 根台账 §三 段锚文本
FULL_DOMAINS = [
    ("A1", "F0800",  "满账封账", "UNX-A1（AI-01"),
    ("B3", "F6397",  "满账封账", "UNX-B3（AI-08"),
    ("C3", "F10400", "满账封账", "UNX-C3（AI-13"),
    ("C5", "F12000", "满账收官", "UNX-C5（AI-15"),
    ("D1", "F12800", "满账封账", "UNX-D1（AI-16"),
]

checks = []


def chk(group, name, ok, detail=""):
    checks.append((group, name, bool(ok), detail))


def main():
    # ---- ① handoff 五域块标量 ----
    h = json.loads(HANDOFF.read_text(encoding="utf-8"))
    total_rows = 0
    for d, anchor, _, _ in FULL_DOMAINS:
        b = h.get("domain_ledger_progress", {}).get(d)
        if b is None:
            chk("①-handoff", f"{d} 块存在", False, "域块缺失")
            continue
        ok_scalar = (b.get("finalized_batches") == 40 and b.get("skeleton_batches") == 0
                     and b.get("rows_locked") == 240000 and b.get("rows_budget") == 240000
                     and b.get("rows_deepened_locked") == 240000
                     and b.get("entries_skeleton") == 0 and b.get("entries_deepened") == 800
                     and len(b.get("finalized_list", [])) == 40)
        chk("①-handoff", f"{d} 块终态八标量（40/0/240,000×3/0/800/list40）", ok_scalar,
            "" if ok_scalar else f"实值 fin={b.get('finalized_batches')} rows={b.get('rows_locked')} entries={b.get('entries_deepened')}")
        total_rows += b.get("rows_locked", 0)

    # ---- ② phase 五域声明 ----
    phase = h.get("phase", "")
    for d, _, kw, _ in FULL_DOMAINS:
        chk("②-phase", f"{d} 域 phase 含『{kw}』",
            f"{d} 满账" in phase and kw in phase, "")

    # ---- ③ 根台账 §三 五域段 ----
    ledger = ROOT_LEDGER.read_text(encoding="utf-8")
    for d, _, _, seg_anchor in FULL_DOMAINS:
        m = re.search(re.escape(seg_anchor) + r"[^）]*）", ledger)
        seg = m.group(0) if m else ""
        ok_seg = bool(seg) and "240,000/240,000" in seg
        chk("③-ledger", f"{d} §三 段满账口径 240,000/240,000", ok_seg,
            "" if ok_seg else "段缺失或口径不符")

    # ---- ④ 五域闭账锚本域批册在册 ----
    for d, anchor, _, _ in FULL_DOMAINS:
        hits = list(DEEPEN.glob(f"{d}-B*.md"))
        found = [f.name for f in hits if anchor in f.read_text(encoding="utf-8")]
        chk("④-anchor", f"{d} 域闭账锚 {anchor} 本域批册在册", len(found) >= 1,
            f"命中 {len(found)} 册" if found else "零命中——锚漂移事故级")

    # ---- ⑤ 里程碑算术 ----
    chk("⑤-milestone", f"Σ rows_locked = 1,200,000（5 域 × 240,000）",
        total_rows == 1200000, f"实和 {total_rows:,}")

    # ---- 汇总 ----
    fails = [c for c in checks if not c[2]]
    print("===== 满账域里程碑对账账（波08-M26 · 1,200,000 行） =====")
    cur = None
    for g, name, ok, detail in checks:
        if g != cur:
            print(f"\n--- {g} ---")
            cur = g
        print(f"  [{'PASS' if ok else 'FAIL'}] {name}" + (f" ｜ {detail}" if detail else ""))
    print(f"\n总计 {len(checks)} 项：PASS {len(checks) - len(fails)} / FAIL {len(fails)}")
    if fails:
        print("\nverdict: 里程碑对账存在漂移——逐处定位归属域会话勘误留痕")
        sys.exit(1)
    print("\nverdict: 五满账域终态全一致——A1/B3/C3/C5/D1 各 240,000 行满账封账，"
          "全域 1,200,000/9,600,000 行里程碑（12.5%）正式对账在案")
    sys.exit(0)


if __name__ == "__main__":
    main()
