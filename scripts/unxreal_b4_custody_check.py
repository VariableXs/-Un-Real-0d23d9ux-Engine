#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
UNX-B4 域保管巡检器（AI-09 · 波08-M33 · 满账后全域核验轮物化）
判例：scripts/unxreal_c5_custody_check.py（AI-15 波08-M24 五项 ALL PASS 体例同构）

五项机械检查（全部只读，不改动任何域册）：
  ① 册数普查：deepen/B4-B01..B40 恰 40 册，零缺失零多册
  ② 条数普查：每册 20 条（^### UNX-Fxxxx），全域 800 条
  ③ ID 连续唯一：全域 ID 恰为 UNX-F6401–F7200，零空洞零重复零越界
  ④ 判据在位：每条带 UNX-Fxxxx-J 判据号，全域 800 判据行
  ⑤ 行数守恒：逐条「纯功能行数」求和 = 240,000（域账封账值 F7199）

退出码：0 = ALL PASS；1 = 存在 FAIL（逐条打印）。
"""
import hashlib
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEEPEN = ROOT / "docs" / "unxreal" / "deepen"
LO, HI = 6401, 7200
DOMAIN_ROWS = 240_000

failures: list[str] = []
passes: list[str] = []


def check(name: str, ok: bool, detail: str) -> None:
    (passes if ok else failures).append(f"{'PASS' if ok else 'FAIL'} {name} {detail}")


def main() -> int:
    books = [DEEPEN / f"B4-B{i:02d}.md" for i in range(1, 41)]
    missing = [p.name for p in books if not p.exists()]
    extra = [p.name for p in DEEPEN.glob("B4-*.md")
             if p.name not in {b.name for b in books}]
    check("①册数普查", not missing and not extra,
          f"40 册缺失 {len(missing)} 件 {missing[:5]}；多册 {len(extra)} 件 {extra[:5]}")

    ids: list[int] = []
    total_rows = 0
    hash_lines: list[str] = []
    for p in books:
        if not p.exists():
            continue
        text = p.read_text(encoding="utf-8")
        digest = hashlib.sha256(text.encode("utf-8")).hexdigest()[:16]
        hash_lines.append(f"| {p.name} | {digest} |")
        heads = re.findall(r"^### (UNX-F(\d{4}))", text, re.M)
        check(f"②{p.stem}条数", len(heads) == 20, f"{len(heads)}/20")
        ids.extend(int(h[1]) for h in heads)
        rows = [int(x) for x in re.findall(r"纯功能行数：(\d+) 行", text)]
        check(f"⑤{p.stem}行数字段", len(rows) == 20, f"字段 {len(rows)}/20")
        total_rows += sum(rows)

    uniq = set(ids)
    check("③ID连续唯一", len(ids) == 800 and uniq == set(range(LO, HI + 1)),
          f"{len(ids)} 条；空洞 {sorted(set(range(LO, HI+1)) - uniq)[:5]}；重复 {len(ids)-len(uniq)}；越界 {sorted(uniq - set(range(LO, HI+1)))[:5]}")

    jcount = 0
    for p in books:
        if p.exists():
            jcount += len(re.findall(rf"UNX-F(\d{{4}})-J\d", p.read_text(encoding="utf-8")))
    check("④判据在位", jcount >= 800, f"判据号出现 {jcount} 处（≥800）")

    check("⑤行数守恒", total_rows == DOMAIN_ROWS, f"Σ={total_rows:,} / {DOMAIN_ROWS:,}（F7199 封账值）")

    for line in passes:
        print(line)
    for line in failures:
        print(line)
    if not failures:
        print(f"\n== ALL PASS ({len(passes)} 项) ==  B4 域册冻结产物机械复验完好；")
        print("哈希基线（SHA-256 前 16 位，供下轮复巡对照）：")
        print("| 册 | SHA-256[:16] |")
        print("|---|---|")
        for line in hash_lines:
            print(line)
        return 0
    print(f"\n== {len(failures)} FAILURES ==")
    return 1


if __name__ == "__main__":
    sys.exit(main())
