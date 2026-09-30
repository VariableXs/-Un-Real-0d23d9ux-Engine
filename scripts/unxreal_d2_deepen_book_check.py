#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""UNX-D2 深化册单册/多册断言（B16–B40 全段，AI-17 域满账扩表版）。

校验项（逐册）：
  1. 分条数 = 20（### UNX-Fxxxxx · 标题）
  2. 每条判据行（判据：UNX-Fxxxxx-J1 ）
  3. 每条状态：[已深化]
  4. 每条行数声明（纯功能行数：N 行（A + B + C；测试段不计））且分解求和 = N
  5. ID 连续且落在登记段
  6. 批行数求和 = 登记值
  7. 六要素齐备：定位/语义边界/依赖与嫁接源/风险与回退/与现存内核衔接点/与 Windows 对照
  8. 每条非空白字符数 >= 300

用法：
    python scripts/unxreal_d2_deepen_book_check.py 17          # 单册
    python scripts/unxreal_d2_deepen_book_check.py 16 17 18    # 多册
    python scripts/unxreal_d2_deepen_book_check.py all         # 全部已落盘册
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEEPEN = ROOT / "docs" / "unxreal" / "deepen"

# 批号 -> (ID 起, ID 止, 登记纯功能行数)
BOOKS = {
    16: (13101, 13120, 5800),
    17: (13121, 13140, 5700),
    18: (13141, 13160, 5600),
    19: (13161, 13180, 5500),
    20: (13181, 13200, 5400),
    21: (13201, 13220, 5300),
    22: (13221, 13240, 5400),
    23: (13241, 13260, 5500),
    24: (13261, 13280, 5600),
    25: (13281, 13300, 5700),
    26: (13301, 13320, 5500),
    27: (13321, 13340, 5400),
    28: (13341, 13360, 5600),
    29: (13361, 13380, 5800),
    30: (13381, 13400, 5800),
    31: (13401, 13420, 7230),
    32: (13421, 13440, 7230),
    33: (13441, 13460, 7230),
    34: (13461, 13480, 7230),
    35: (13481, 13500, 7230),
    36: (13501, 13520, 7230),
    37: (13521, 13540, 7230),
    38: (13541, 13560, 7230),
    39: (13561, 13580, 7230),
    40: (13581, 13600, 7230),
}

MIN_CHARS = 300
ELEMENTS = ["定位", "语义边界", "依赖与嫁接源", "风险与回退", "与现存内核衔接点", "与 Windows 对照"]


def check_book(no: int):
    id_lo, id_hi, declared = BOOKS[no]
    path = DEEPEN / f"D2-B{no}.md"
    errs = []
    if not path.exists():
        return [f"B{no}: 文件不存在 {path}"]
    text = path.read_text(encoding="utf-8")
    entries = re.split(r"(?=^### UNX-F\d+ · )", text, flags=re.M)
    entries = [e for e in entries if e.startswith("### UNX-F")]
    if len(entries) != 20:
        errs.append(f"B{no}: 分条数 {len(entries)} != 20")
    total = 0
    ids = []
    for entry in entries:
        m = re.match(r"### UNX-F(\d+) · ", entry)
        if not m:
            errs.append(f"B{no}: 条目标题格式异常: {entry[:40]!r}")
            continue
        fid = int(m.group(1))
        ids.append(fid)
        tag = f"F{fid}"
        if not re.search(rf"判据：UNX-F{fid}-J1 ", entry):
            errs.append(f"{tag}: 判据行缺失/格式错")
        if "状态：[已深化]" not in entry:
            errs.append(f"{tag}: 状态非 [已深化]")
        rm = re.search(r"纯功能行数：(\d+) 行（(.+?)；测试段不计）", entry)
        if not rm:
            errs.append(f"{tag}: 行数声明缺失/格式错")
            continue
        declared_entry = int(rm.group(1))
        total += declared_entry
        parts = []
        for seg in re.split(r"\s*\+\s*", rm.group(2)):
            m2 = re.search(r"(\d+)\s*$", seg.strip())
            if not m2:
                errs.append(f"{tag}: 分解段无行数「{seg}」")
                continue
            parts.append(int(m2.group(1)))
        if sum(parts) != declared_entry:
            errs.append(f"{tag}: 分解求和 {sum(parts)} != 声明 {declared_entry}")
        for el in ELEMENTS:
            if el not in entry:
                errs.append(f"{tag}: 要素缺失「{el}」")
        body = re.sub(r"\s", "", entry)
        if len(body) < MIN_CHARS:
            errs.append(f"{tag}: 字数 {len(body)} < {MIN_CHARS}")
    if ids and (ids[0] != id_lo or ids[-1] != id_hi or ids != list(range(id_lo, id_hi + 1))):
        head = f"首 {ids[0]} 末 {ids[-1]}" if ids else "空"
        errs.append(f"B{no}: ID 不连续或越界（{head}，期望 {id_lo}-{id_hi}）")
    if total != declared:
        errs.append(f"B{no}: 批求和 {total} != 登记 {declared}")
    return errs


def main():
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        sys.exit(2)
    if args[0] == "all":
        nos = [n for n in sorted(BOOKS) if (DEEPEN / f"D2-B{n}.md").exists()]
    else:
        nos = [int(a) for a in args]
    all_errs = []
    for n in nos:
        errs = check_book(n)
        id_lo, id_hi, declared = BOOKS[n]
        status = "PASS" if not errs else "FAIL"
        print(f"B{n:02d} F{id_lo}-F{id_hi} 登记 {declared} 行：{status} ({len(errs)} err)")
        all_errs.extend(errs)
    if all_errs:
        print("\n".join("  - " + e for e in all_errs))
        sys.exit(1)
    print(f"\n全部 {len(nos)} 册断言通过。")


if __name__ == "__main__":
    main()
