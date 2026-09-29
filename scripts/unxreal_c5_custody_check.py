#!/usr/bin/env python3
"""UNX-C5 域保管巡检器（AI-15 物化 · 波 08-M24）

对 UNX-C5（用户态生态嫁接，F11201–F12000，40 批 800 条 240,000 行）满账收官后的
冻结产物做一键保管复验。五项检查与收官断言同源（域收官总报告 §一/§二/§五）：

  1. 哈希基线   80 册 SHA-256 前 16 位对照收官报告 §五 F11975 固化基线
  2. 行数守恒   40 批逐条求和（骨架册两行式条目全量 findall——勿用首条匹配）
                对照 §二 全表，Σ=240,000，每批恰 20 条
  3. 双册同构   骨架册与深化册逐条行数按判据 ID 一一对位相等；
                深化册两种实证记账形态均覆盖：
                a)「纯功能行数：N 行（分解式）」——分解求和=N 且与骨架册对位
                b) B33–B40「- 判据行数：分解式」无总声明——总声明取分解求和对位
  4. 键普查     F11201–F12000 在骨架册全域恰 800 键零重零缺、深化册同（F11985 口径，
                以 ^### UNX-F##### 标题行为结构位，正文引用不计数）
  5. 收官标志   F12000 域收官标志条在 B40 深化册在位

用法：  python scripts/unxreal_c5_custody_check.py   （任意工作目录可执行，路径以本文件定位）
退出码：0 = 五项全过；1 = 存在失配（逐项列出，不静默）
"""

import hashlib
import re
import sys
from collections import Counter
from pathlib import Path

BASE = Path(__file__).resolve().parent.parent
BATCH_DIR = BASE / "docs" / "unxreal" / "batches"
DEEPEN_DIR = BASE / "docs" / "unxreal" / "deepen"
REPORT = BASE / "docs" / "unxreal" / "reports" / "UNX-C5-domain-closeout-report.md"

N_BATCHES = 40          # C5 域 40 批
ENTRIES_PER_BATCH = 20  # 每批 20 条（两行式条目）
ROW_TOTAL = 240_000     # 域预算满额
ID_FIRST, ID_LAST = 11201, 12000  # 判据 ID 连续区间

TITLE_RE = re.compile(r"^### UNX-F(\d{5})\b", re.M)

# 分节解析（波 08-M24 第三轮修订——先证伪检查器再动手，三种实证记账形态见 parse_entries）：
#   ID 锚：### UNX-F##### 标题行（F11985 结构位口径）
#   共置行 ID：行内 UNX-F#####-J1（与行数同行时绑定置信最高）
#   变异族 B 锚：^- 判据行数： bullet 行首（B39 散文提及「判据行数」非声明行，须锚定防误捕）
ID_RE = re.compile(r"UNX-F(\d{5})-J1")
ROWS_RE = re.compile(r"纯功能行数：(\d+(?:,\d{3})*)")
BREAKDOWN_RE = re.compile(r"纯功能行数：\d+(?:,\d{3})* 行（([^）]*)）")
JU_ROWS_RE = re.compile(r"^-\s*判据行数：(.+)$", re.M)


def parse_entries(text: str):
    """按 ### UNX-F##### 标题分节解析 {判据 ID: (声明行数, 分解式或 None)}。

    三种实证记账形态（M24 修订，不假设字段序/同线性）：
      a) ID 与行数同行共置（骨架册全量 / 深化册 B02–B32）——以行内 ID 为准
      b) ID 仅在标题行、行数在「域/批」行（深化册 B01/F11208 跨行变异）——绑定标题 ID
      c) 仅「- 判据行数：A n + B n + C n；测试段不计」无总声明（深化册 B33–B40 全量）
         ——总声明取分解式求和，与骨架册总声明同构核对
    同节多行数行时首见生效（防册尾脚注误覆盖）；同行共置优先级最高（见即断）。
    """
    out = {}
    for sec in re.split(r"(?=^### UNX-F\d{5}\b)", text, flags=re.M):
        mtitle = TITLE_RE.match(sec)
        if not mtitle:
            continue
        fid = mtitle.group(1)
        rows, bk_text, co = None, None, None
        for line in sec.splitlines():
            if "纯功能行数" not in line:
                continue
            mrow = ROWS_RE.search(line)
            if not mrow:
                continue
            if ID_RE.search(line):  # 同行共置：最高置信，见即断
                co = (int(mrow.group(1).replace(",", "")), BREAKDOWN_RE.search(line))
                break
            if rows is None:  # 跨行行数行：首见生效
                rows = int(mrow.group(1).replace(",", ""))
                mbk = BREAKDOWN_RE.search(line)
                bk_text = mbk.group(1) if mbk else None
        if co is not None:
            rows = co[0]
            bk_text = co[1].group(1) if co[1] else None
        elif rows is None:  # 变异族 B：判据行数分解式，总声明=求和（分解文本如实存档）
            mju = JU_ROWS_RE.search(sec)
            if mju:
                bk_text = mju.group(1).strip()
                rows = breakdown_sum(bk_text)
        if rows is None:
            continue
        out[fid] = (rows, bk_text)
    return out


def sha16(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()[:16]


def batch_path(kind: str, no: int) -> Path:
    name = (f"UNX-C5-B{no:02d}.md" if kind == "skeleton" else f"C5-B{no:02d}.md")
    return (BATCH_DIR if kind == "skeleton" else DEEPEN_DIR) / name


def parse_baseline(text: str):
    """从收官报告 §五 提取 F11975 固化基线（骨架 40 + 深化 40，SHA-256 前 16 位）。"""
    def grab(section: str):
        return {m.group(1): m.group(2) for m in re.finditer(r"B(\d{2}) ([0-9a-f]{16})", section)}

    skel_sec = text.split("骨架册 40 册")[1].split("深化册 40 册")[0]
    deep_sec = text.split("深化册 40 册")[1].split("共享协调文件")[0]
    return grab(skel_sec), grab(deep_sec)


def parse_report_table(text: str):
    """从收官报告 §二 提取逐批行数声明（| B01 | … | 20 | 5,240 | …）。"""
    sec = text.split("## 二、全域账面")[1].split("## 三、")[0]
    return {int(m.group(1)): int(m.group(2).replace(",", ""))
            for m in re.finditer(r"\| B(\d{2}) \| F\d+–F\d+ \| \d+ \| ([\d,]+) \|", sec)}


def breakdown_sum(content: str) -> int:
    """深化册分解式求和：'A 110 + B 100 + C 90；测试段不计' -> 310（尾注段剔除）。"""
    main = content.split("；")[0]
    total = 0
    for part in main.split("+"):
        nums = re.findall(r"(\d+)\s*$", part.strip())
        if nums:
            total += int(nums[0])
    return total


def check_hash_baseline(report_text: str, failures: list) -> None:
    exp_skel, exp_deep = parse_baseline(report_text)
    if len(exp_skel) != N_BATCHES or len(exp_deep) != N_BATCHES:
        failures.append(f"[哈希基线] 报告基线解析不全：骨架 {len(exp_skel)}/深化 {len(exp_deep)}（期望各 {N_BATCHES}）")
        return
    mismatch = []
    for kind, expected in (("skeleton", exp_skel), ("deepen", exp_deep)):
        for no_str, exp in sorted(expected.items()):
            path = batch_path(kind, int(no_str))
            if not path.exists():
                mismatch.append(f"{path.name} 缺失")
            elif sha16(path) != exp:
                mismatch.append(f"{path.name} 现值 {sha16(path)} ≠ 基线 {exp}")
    if mismatch:
        failures.append("[哈希基线] 失配/缺失 " + str(len(mismatch)) + " 件：" + "；".join(mismatch[:8]))
    else:
        print(f"  ✓ 哈希基线：{N_BATCHES * 2}/{N_BATCHES * 2} 册 ALL MATCH（F11975 基线零失真）")


def check_rows(report_text: str, failures: list) -> dict:
    expected = parse_report_table(report_text)
    if len(expected) != N_BATCHES:
        failures.append(f"[行数守恒] 报告 §二 全表解析不全：{len(expected)}/{N_BATCHES} 批")
    totals, per_rows = 0, {}
    for no in range(1, N_BATCHES + 1):
        entries = parse_entries(batch_path("skeleton", no).read_text(encoding="utf-8"))
        if len(entries) != ENTRIES_PER_BATCH:
            failures.append(f"[行数守恒] B{no:02d} 骨架条目数 {len(entries)} ≠ {ENTRIES_PER_BATCH}")
        per_rows[no] = {fid: rows for fid, (rows, _) in entries.items()}
        s = sum(per_rows[no].values())
        totals += s
        if no in expected and s != expected[no]:
            failures.append(f"[行数守恒] B{no:02d} 逐条求和 {s} ≠ 报告声明 {expected[no]}")
    if totals != ROW_TOTAL:
        failures.append(f"[行数守恒] 全域 Σ={totals} ≠ {ROW_TOTAL}（diff={totals - ROW_TOTAL}）")
    else:
        print(f"  ✓ 行数守恒：Σ={totals} diff=0，{N_BATCHES}/{N_BATCHES} 批级与 §二 全表零偏离（每批 {ENTRIES_PER_BATCH} 条）")
    return per_rows


def check_dual_book(per_rows: dict, failures: list) -> None:
    bad = []
    for no in range(1, N_BATCHES + 1):
        entries = parse_entries(batch_path("deepen", no).read_text(encoding="utf-8"))
        deepen = {fid: rows for fid, (rows, _) in entries.items()}
        if len(deepen) != ENTRIES_PER_BATCH:
            bad.append(f"B{no:02d} 深化条目数 {len(deepen)} ≠ {ENTRIES_PER_BATCH}")
        for fid, (rows, breakdown) in entries.items():
            if breakdown is not None and breakdown_sum(breakdown) != rows:
                bad.append(f"B{no:02d}/F{fid} 分解求和 {breakdown_sum(breakdown)} ≠ 声明 {rows}")
        for fid, rows in per_rows[no].items():
            if fid not in deepen:
                bad.append(f"B{no:02d}/F{fid} 深化册缺行")
            elif deepen[fid] != rows:
                bad.append(f"B{no:02d}/F{fid} 双册行数不齐（骨架 {rows} vs 深化 {deepen[fid]}）")
    if bad:
        failures.append("[双册同构] " + str(len(bad)) + " 处：" + "；".join(bad[:8]))
    else:
        print(f"  ✓ 双册同构：{N_BATCHES * ENTRIES_PER_BATCH} 对逐条行数一一对位相等，分解式求和逐条守恒")


def check_keys(failures: list) -> None:
    expected = {str(n) for n in range(ID_FIRST, ID_LAST + 1)}
    for kind, label in (("skeleton", "骨架册"), ("deepen", "深化册")):
        found = []
        for no in range(1, N_BATCHES + 1):
            found += TITLE_RE.findall(batch_path(kind, no).read_text(encoding="utf-8"))
        counter = Counter(found)
        dup = {k: v for k, v in counter.items() if v != 1}
        missing = sorted(expected - set(counter))
        extra = sorted(set(counter) - expected)
        if dup or missing or extra:
            failures.append(
                f"[键普查] {label} 异常：重号 {len(dup)} 缺号 {len(missing)} 越界 {len(extra)}"
                + (f"；样例 重{list(dup)[:3]} 缺{missing[:3]} 越{extra[:3]}" if dup or missing or extra else ""))
        elif kind == "deepen":
            print(f"  ✓ 键普查：F{ID_FIRST}–F{ID_LAST} 共 {len(expected)} 键 × 骨架/深化各 1 次，零重零缺零越界（F11985 口径）")


def check_finale(failures: list) -> None:
    text = batch_path("deepen", N_BATCHES).read_text(encoding="utf-8")
    if f"F{ID_LAST}" not in text:
        failures.append(f"[收官标志] F{ID_LAST} 域收官标志条不在 B{N_BATCHES} 深化册")
    else:
        print(f"  ✓ 收官标志：F{ID_LAST} 域收官标志条在 B{N_BATCHES:02d} 深化册在位")


def main() -> int:
    print(f"UNX-C5 域保管巡检器 · 基准 {REPORT.name}")
    report_text = REPORT.read_text(encoding="utf-8")
    failures: list = []
    check_hash_baseline(report_text, failures)
    per_rows = check_rows(report_text, failures)
    check_dual_book(per_rows, failures)
    check_keys(failures)
    check_finale(failures)
    if failures:
        print(f"\n结论：FAIL —— {len(failures)} 项失配（不静默，逐条处置）")
        for f in failures:
            print("  ✗", f)
        return 1
    print("\n结论：ALL PASS —— C5 冻结产物保管完好（80 册零失真/守恒零偏离/键面零异常）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
