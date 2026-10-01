#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
unxreal_b3_hash_recheck.py — B3 账物哈希基线复巡 + AI-90 核签证据链固化（波08-M27，AI-08）

背景判例链：
  - M20：闭账物 §五 43 文件 SHA-256 固化（账物完整性基线）
  - M22：43/43 复巡全过（脚本首跑路径基准错 root cause 留痕——检查器有罪文件无罪）
  - M24–M26：锚号层/记载层/里程碑层三层巡检，43 件哈希未再复算
  - M27 本轮：① 43 件基线周期性复巡（append-only 纪律验证：固化后任何字节级改动即失真）
              ② AI-90 核签证据链十件 SHA-256 固化（核签时完整性验证基线）

Part A：解析闭账物 §五 固化表 43 条 → 逐一复算对照（零失真即 PASS）
Part B：核签证据链十件（closeout×4 + 收官报告 + 里程碑对账账 + 断言链脚本×4）实算固化表输出

路径映射（M22 教训固化）：表内 '../reports/x' → docs/unxreal/reports/x；裸名 → docs/unxreal/deepen/x。
判据输出：exit 0=全绿，exit 1=失真（事故级，须定位改动来源）。
"""
import hashlib
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
LEDGER = REPO / "docs" / "unxreal" / "closeout" / "UNX-B3-NTFS-interop-ledger.md"


def sha256_of(p: Path) -> str:
    return hashlib.sha256(p.read_bytes()).hexdigest()


def resolve_path(name: str) -> Path:
    if name.startswith("../"):
        return REPO / "docs" / "unxreal" / name[3:]
    return REPO / "docs" / "unxreal" / "deepen" / name


def main():
    text = LEDGER.read_text(encoding="utf-8")
    # §五 表体：仅取 "## 五、" 与 "## 六、" 之间的行
    sec5 = text.split("## 五、")[1].split("## 六、")[0]
    rows = re.findall(r"^\| (\S+) \| ([0-9a-f]{64}) \|", sec5, re.M)
    print(f"[Part A] §五 固化表解析：{len(rows)} 条（期望 43）")
    if len(rows) != 43:
        print("[FATAL] 固化表条数≠43——表体解析或固化基线异常")
        sys.exit(2)

    fails = []
    for name, expect in rows:
        p = resolve_path(name)
        if not p.exists():
            fails.append((name, "FILE MISSING"))
            continue
        actual = sha256_of(p)
        if actual != expect:
            fails.append((name, f"MISMATCH expect={expect[:16]}… actual={actual[:16]}…"))

    print(f"[Part A] 复巡结果：{len(rows) - len(fails)}/{len(rows)} PASS")
    for name, why in fails:
        print(f"  [FAIL] {name} — {why}")

    # ---- Part B：核签证据链十件固化 ----
    evidence = [
        ("docs/unxreal/closeout/UNX-B3-NTFS-interop-ledger.md", "闭账物主件 v1.0"),
        ("docs/unxreal/closeout/UNX-B3-ledger-acceptance-dryrun.md", "验收预演记录 8+1"),
        ("docs/unxreal/closeout/UNX-B3-anchor-audit.md", "锚号全量核对账"),
        ("docs/unxreal/closeout/UNX-B3-consistency-audit.md", "终态记载一致性巡检账"),
        ("docs/unxreal/reports/UNX-B3-domain-closeout-report.md", "域收官报告"),
        ("docs/unxreal/reports/UNX-full-domains-milestone-audit.md", "满账域里程碑对账账"),
        ("scripts/unxreal_b3_anchor_audit.py", "断言链·锚号核对器"),
        ("scripts/unxreal_b3_consistency_check.py", "断言链·记载巡检器"),
        ("scripts/unxreal_b3_hash_recheck.py", "断言链·哈希复巡器（本脚本，自指排除说明：其入库后哈希由 git 对象承载，本表值=落库前工作区版本）"),
        ("docs/unxreal/deepen/finalize_check_domain.py", "断言链·域级 finalize 78 项"),
    ]
    print(f"\n[Part B] AI-90 核签证据链固化表（{len(evidence)} 件实算）")
    for rel, role in evidence:
        p = REPO / rel
        if p.exists():
            print(f"| {rel} | {sha256_of(p)} | {role} |")
        else:
            print(f"| {rel} | MISSING | {role} |")

    if fails:
        print("\nverdict: 账物失真——事故级，须定位改动来源会话并勘误留痕（固化物不回改）")
        sys.exit(1)
    print("\nverdict: 43 件基线复巡零失真——M20 固化基线在 M22–M26 六轮高并发后仍字节级完好，"
          "append-only 纪律实证维持；核签证据链十件固化表已输出")
    sys.exit(0)


if __name__ == "__main__":
    main()
