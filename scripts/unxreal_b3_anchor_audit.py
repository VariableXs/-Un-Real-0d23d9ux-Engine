#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
unxreal_b3_anchor_audit.py — B3 域派生物锚号全量核对器（波08-M24，AI-08）

背景判例链：
  - M23 预演第 3 项抽查 19 锚抓获 F6394 批归属转述笔误（报告标注 B39，权威源实为 B3/B40）
  - M23 教训入档："派生报告物锚号引用须 grep 实证禁记忆转述"
  - 本脚本 = 该教训的全量清偿：对收官报告 + 闭账物全部 F6xxx 锚号逐一实证

双层判定：
  L1 在册性：B3 区间（F5601–F6400）锚号必须出现在 deepen/B3-B01..B40 至少一册
  L2 批归属：派生物中与锚号同一行的显式 Bxx 声明必须与权威源实际批册一致

路径基准：以 __file__ 定位仓库根（M22 巡检路径教训——不依赖 cwd）。
判据输出：逐锚 PASS/FAIL 清单 + 汇总；exit 0=全绿（或仅已知笔误留痕），exit 1=新 FAIL。
"""
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
DEEPEN = REPO / "docs" / "unxreal" / "deepen"
B3_RANGE = (5601, 6400)

# 被核对的两份派生物（B3 域固化闭账物 + 域收官报告）
DERIVED = [
    REPO / "docs" / "unxreal" / "closeout" / "UNX-B3-NTFS-interop-ledger.md",
    REPO / "docs" / "unxreal" / "reports" / "UNX-B3-domain-closeout-report.md",
]

ANCHOR_RE = re.compile(r"F(6[0-9]{3})")
# L2 声明式一：锚号在前——"F6394（B39）" / "F6201∈B31"（首轮缺陷修正：补 ∈ 符号，
#   该符号漏检导致收官报告双锚保真句 4 处声明零核查；斜杠等分隔符不构成声明，
#   "F6201∈B31 / F6400∈B40" 中 B31 不得错配 F6400——检查器有罪文件无罪，M22 判例）
DECL_AFTER_RE = re.compile(r"F(6[0-9]{3})\s*[∈(（]\s*B(\d{1,2})")
# L2 声明式二：批号在前——仅全角括号构成立（实证用例 "B37（F6321"）；
#   半角 / · - 等分隔符一律不算，防 "B31 / F6400" 交叉假阳性
DECL_BEFORE_RE = re.compile(r"B(\d{1,2})\s*[(（]\s*F(6[0-9]{3})")


def build_authority():
    """grep 实证：B3 域 40 册逐册建 锚号→批册集合 索引。"""
    index = {}
    files = sorted(DEEPEN.glob("B3-B*.md"))
    if len(files) != 40:
        print(f"[FATAL] deepen/B3-B*.md 期望 40 册，实得 {len(files)}——权威源不全，拒绝核对")
        sys.exit(2)
    for f in files:
        m = re.search(r"B3-B(\d{2})\.md$", f.name)
        batch = int(m.group(1))
        text = f.read_text(encoding="utf-8")
        for a in ANCHOR_RE.findall(text):
            index.setdefault(int(a), set()).add(batch)
    return index


def main():
    authority = build_authority()
    print(f"[authority] B3 权威源 40 册建索引完成：{len(authority)} 个唯一锚号在册")

    failures = []      # (file, line_no, line, anchor, detail)
    known = []         # M23 已勘误留痕的 F6394 笔误（预期复现，不计新 FAIL）
    l2_detail = []     # (rel, line_no, line, anchor, batch, ok) L2 逐条明细
    stats = {"anchors": set(), "l1_pass": 0, "l1_fail": 0,
             "l2_checked": 0, "l2_pass": 0, "l2_fail": 0, "out_of_range": 0}

    for path in DERIVED:
        rel = str(path.relative_to(REPO)).replace("\\", "/")
        lines = path.read_text(encoding="utf-8").splitlines()
        for i, line in enumerate(lines, 1):
            for a_str in ANCHOR_RE.findall(line):
                a = int(a_str)
                stats["anchors"].add((rel, a))
                lo, hi = B3_RANGE
                if not (lo <= a <= hi):
                    stats["out_of_range"] += 1   # 跨域引用（合法），跳过 L1
                    continue
                # L1 在册性
                if a not in authority:
                    failures.append((rel, i, line.strip(), a_str,
                                     "L1-FAIL：B3 区间锚号在 40 册权威源零命中"))
                    stats["l1_fail"] += 1
                else:
                    stats["l1_pass"] += 1
                # L2 批归属（同一行双向声明）
                decls = set()
                for _, b in DECL_AFTER_RE.findall(line):
                    if int(_) == a:
                        decls.add(int(b))
                for b, _ in DECL_BEFORE_RE.findall(line):
                    if int(_) == a:
                        decls.add(int(b))
                for b in decls:
                    stats["l2_checked"] += 1
                    ok = b in authority.get(a, set())
                    l2_detail.append((rel, i, line.strip(), a, b, ok))
                    if not ok:
                        rec = (rel, i, line.strip(), a_str,
                               f"L2-FAIL：报告声明 B{b:0>2}，权威源实际在 "
                               f"{sorted(authority.get(a, set()))}")
                        if a == 6394:
                            known.append(rec)   # M23 勘误留痕已知
                        else:
                            failures.append(rec)
                        stats["l2_fail"] += 1
                    else:
                        stats["l2_pass"] += 1

    # ---- 汇总 ----
    total = len(stats["anchors"])
    print("\n===== B3 派生物锚号全量核对账（波08-M24） =====")
    print(f"派生物：{len(DERIVED)} 份（闭账物 + 域收官报告）")
    print(f"唯一锚号合计：{total}（其中域外引用 {stats['out_of_range']} 跳过 L1）")
    print(f"L1 在册性：PASS {stats['l1_pass']} / FAIL {stats['l1_fail']}")
    print(f"L2 批归属：核查 {stats['l2_checked']} 处声明 —— PASS {stats['l2_pass']} / "
          f"FAIL {stats['l2_fail']}（含 M23 已勘误留痕 F6394 {len(known)} 处）")

    if l2_detail:
        print("\n--- L2 批归属声明逐条明细 ---")
        for rel, i, line, a, b, ok in l2_detail:
            mark = "PASS" if ok else "FAIL"
            print(f"  [{mark}] {rel}:{i} F{a}←B{b:0>2} ｜ {line[:80]}")

    if known:
        print(f"\n--- M23 已勘误留痕复现（不计新 FAIL）×{len(known)} ---")
        for rel, i, line, a, d in known:
            print(f"  [known] {rel}:{i} F{a} — {d}")

    if failures:
        print(f"\n--- 新增 FAIL ×{len(failures)} ---")
        for rel, i, line, a, d in failures:
            print(f"  [FAIL] {rel}:{i} F{a} — {d}")
            print(f"         原文：{line[:120]}")
        print("\nverdict: 存在新增锚号笔误——须勘误留痕（固化物不回改）")
        sys.exit(1)

    print("\nverdict: 双层核对全绿——除 M23 已留痕 F6394 外零新增笔误，"
          "M23 教训『派生报告物锚号引用须 grep 实证』全量清偿完成")
    sys.exit(0)


if __name__ == "__main__":
    main()
