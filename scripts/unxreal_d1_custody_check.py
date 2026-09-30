#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
UNX-D1 域保管巡检器（AI-16 · 波08-M34 · 满账后全域核验轮物化）
判例：scripts/unxreal_b4_custody_check.py（AI-09 波08-M33 五项 ALL PASS 体例同构）
      + scripts/unxreal_c5_custody_check.py（AI-15 波08-M24 五项 ALL PASS 体例同构）

六项机械检查（全部只读，不改动任何域册）：
  ① 册数普查：deepen/D1-B01..B40 恰 40 册，零缺失零多册
  ② 条数普查：每册 20 条（^### UNX-Fxxxx），全域 800 条
  ③ ID 连续唯一：全域 ID 恰为 UNX-F12001–F12800，零空洞零重复零越界
  ④ 判据在位：每条带 UNX-Fxxxx-J 判据号，全域 ≥800 判据行
  ⑤ 行数守恒：逐条「纯功能行数」求和 = 240,000（域账封账值 F12800 三段加法 86,740+90,000+63,260）
  ⑥ 双册同构：batches/UNX-D1-B01..B40 骨架册 40 件与深化册逐批条目 ID 对位一致

退出码：0 = ALL PASS；1 = 存在 FAIL（逐条打印）。
用法：
  python scripts/unxreal_d1_custody_check.py            # 只巡检
  python scripts/unxreal_d1_custody_check.py --baseline # 巡检全绿后写哈希基线 closeout/UNX-D1-custody-recheck-20260930.md
"""
import hashlib
import re
import sys
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEEPEN = ROOT / "docs" / "unxreal" / "deepen"
BATCHES = ROOT / "docs" / "unxreal" / "batches"
CLOSEOUT = ROOT / "docs" / "unxreal" / "closeout"
LO, HI = 12_001, 12_800
DOMAIN_ROWS = 240_000

failures: list[str] = []
passes: list[str] = []


def check(name: str, ok: bool, detail: str) -> None:
    (passes if ok else failures).append(f"{'PASS' if ok else 'FAIL'} {name} {detail}")


def main() -> int:
    books = [DEEPEN / f"D1-B{i:02d}.md" for i in range(1, 41)]
    batch_books = [BATCHES / f"UNX-D1-B{i:02d}.md" for i in range(1, 41)]

    # ① 册数普查（深化册）
    missing = [p.name for p in books if not p.exists()]
    known = {b.name for b in books}
    extra = [p.name for p in DEEPEN.glob("D1-*.md") if p.name not in known]
    check("①册数普查(深化册)", not missing and not extra,
          f"40 册缺失 {len(missing)} 件 {missing[:5]}；多册 {len(extra)} 件 {extra[:5]}")

    # ⑥ 册数普查（骨架批册面）
    b_missing = [p.name for p in batch_books if not p.exists()]
    b_known = {b.name for b in batch_books}
    b_extra = [p.name for p in BATCHES.glob("UNX-D1-B*.md") if p.name not in b_known]
    check("⑥a批册面普查", not b_missing and not b_extra,
          f"40 册缺失 {len(b_missing)} 件 {b_missing[:5]}；多册 {len(b_extra)} 件 {b_extra[:5]}")

    ids: list[int] = []
    total_rows = 0
    hash_lines: list[str] = []
    for p in books:
        if not p.exists():
            continue
        text = p.read_text(encoding="utf-8")
        digest = hashlib.sha256(text.encode("utf-8")).hexdigest()[:16]
        hash_lines.append((p.name, digest))
        heads = re.findall(r"^### (UNX-F(\d+))\b", text, re.M)
        check(f"②{p.stem}条数", len(heads) == 20, f"{len(heads)}/20")
        ids.extend(int(h[1]) for h in heads)
        rows = [int(x) for x in re.findall(r"纯功能行数：(\d+) 行", text)]
        check(f"⑤{p.stem}行数字段", len(rows) == 20, f"字段 {len(rows)}/20")
        total_rows += sum(rows)

    uniq = set(ids)
    check("③ID连续唯一", len(ids) == 800 and uniq == set(range(LO, HI + 1)),
          f"{len(ids)} 条；空洞 {sorted(set(range(LO, HI + 1)) - uniq)[:5]}；"
          f"重复 {len(ids) - len(uniq)}；越界 {sorted(uniq - set(range(LO, HI + 1)))[:5]}")

    jcount = 0
    for p in books:
        if p.exists():
            jcount += len(re.findall(r"UNX-F(\d{4,5})-J\d", p.read_text(encoding="utf-8")))
    check("④判据在位", jcount >= 800, f"判据号出现 {jcount} 处（≥800）")

    check("⑤行数守恒", total_rows == DOMAIN_ROWS,
          f"Σ={total_rows:,} / {DOMAIN_ROWS:,}（F12800 封账值；86,740+90,000+63,260 三段加法账）")

    # ⑥b 双册同构：骨架批册条目 ID 集合与深化册逐批对位
    drift: list[str] = []
    deep_ids_by_batch: dict[int, list[int]] = {}
    for p in books:
        if p.exists():
            m = re.search(r"B(\d{2})", p.stem)
            deep_ids_by_batch[int(m.group(1))] = [
                int(x) for x in re.findall(r"^### UNX-F(\d+)\b", p.read_text(encoding="utf-8"), re.M)]
    for p in batch_books:
        if not p.exists():
            continue
        m = re.search(r"B(\d{2})", p.stem)
        bn = int(m.group(1))
        skel_ids = [int(x) for x in re.findall(r"^### UNX-F(\d+)\b", p.read_text(encoding="utf-8"), re.M)]
        if skel_ids != deep_ids_by_batch.get(bn):
            drift.append(f"B{bn:02d}(骨架{len(skel_ids)}/深化{len(deep_ids_by_batch.get(bn, []))})")
    check("⑥b双册同构", not drift, f"40 批骨架↔深化逐批 ID 对位；漂移批 {drift[:5] or '无'}")

    for line in passes:
        print(line)
    for line in failures:
        print(line)

    if not failures:
        print(f"\n== ALL PASS ({len(passes)} 项) ==  D1 域册冻结产物机械复验完好；")
        print("哈希基线（SHA-256 前 16 位，供下轮复巡对照）：")
        print("| 册 | SHA-256[:16] |")
        print("|---|---|")
        for name, digest in hash_lines:
            print(f"| {name} | {digest} |")
        if "--baseline" in sys.argv:
            CLOSEOUT.mkdir(parents=True, exist_ok=True)
            out = CLOSEOUT / "UNX-D1-custody-recheck-20260930.md"
            body = [
                "# UNX-D1 域保管巡检基线 v1.0（AI-16 · 波08-M34）",
                "",
                f"> 生成工具：scripts/unxreal_d1_custody_check.py --baseline｜生成日期：{date.today().isoformat()}｜"
                "判例：UNX-B4-custody-recheck-20260930.md / C5 F11975 哈希基线同构",
                "> 用途：下轮 D1 域保管复巡以本表逐册 SHA-256 前 16 位对照，ALL MATCH 即域册零失真零缺失。",
                f"> 巡检结果：{len(passes)} 项 ALL PASS exit=0（六项检查①–⑥）；行数守恒 Σ=240,000=86,740+90,000+63,260（F12800 封账值）。",
                "",
                "| 册 | SHA-256[:16] |",
                "|---|---|",
            ]
            body += [f"| {n} | {d} |" for n, d in hash_lines]
            out.write_text("\n".join(body) + "\n", encoding="utf-8")
            print(f"\n哈希基线已落盘：{out}")
        return 0
    print(f"\n== {len(failures)} FAILURES ==")
    return 1


if __name__ == "__main__":
    sys.exit(main())
