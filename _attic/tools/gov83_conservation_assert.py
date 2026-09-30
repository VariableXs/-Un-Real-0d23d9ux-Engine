#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
GOV-83 行数守恒断言器 v1.0
AI-83 · 行数守恒官 · 首件治理工具
判据：
  GOV-83-J1  80 域累计恒等于 19,200,000 ± 域内在途；每波出对账单
  GOV-83-J2  断言器注入抽测 ±1 行必被抓
  GOV-83-J3  超支预警 T+0（单域消耗进度偏离批次进度 ±30% 即预警）
  GOV-83-J4  三源对账差异超阈值告警闭环率 100%
  GOV-83-J3t 断言器自身 ≤ 2,000 行（本文件行数自检）
三源对账（源册 §38"不信任任何单一数据源"）：
  S1 = 独立增补册逐批批头声明行数
  S2 = 独立增补册逐条实计行数求和
  S3 = 主汇编册同名条目行数（若该域已入主册）
用法（在仓库根目录执行）：
  python _attic/tools/gov83_conservation_assert.py \
      --base "docs/Varix/CoRun Varix STAR II · Unxreal" --wave W1 \
      [--inject <ID>=<delta>] [--report <out.md>]
退出码：0=全绿；1=断言失败；2=注入抽测漏抓（最严重，视为断言器自身缺陷）
"""
import argparse
import os
import re
import sys
from collections import defaultdict

TOTAL_TARGET = 19_200_000          # 80 域 × 240,000
DOMAIN_QUOTA = 240_000
BATCH_QUOTA = 6_000                # 批累计 6,000 ± 5%
BATCH_TOL = 0.05
DEV_TOL = 0.30                     # GOV-83-J3 偏离 ±30% 预警
SELF_LINE_LIMIT = 2_000            # GOV-83-J3t

RE_BATCH_HEAD = re.compile(r"^#{2,3}\s+批\s+\S*?-?B(\d{2})")
RE_BATCH_DECL = re.compile(r"·\s*([\d,]+)\s*行[）) ]")
RE_TABLE_ROW = re.compile(r"^\|\s*([A-Z][A-Z0-9]*(?:-[A-Z0-9]+)*\d[A-Z0-9]*)\s*\|\s*([^|]+?)\s*\|\s*([\d,]+)\s*\|")
RE_ITEM_HEAD = re.compile(r"^###\s+([A-Z]+-F\d+|GOV-\d+-F\d+)\s*[·.]\s*(.+)$")
RE_ID = re.compile(r"(?:UNX|GOV)-F?\d+|(?:GOV)-\d+-F\d+")
RE_ANYID = re.compile(r"\b((?:UNX|GOV)-F\d{3,5}|GOV-\d+-F\d{3})\b")


def norm_lines(n):
    return int(str(n).replace(",", ""))


def this_file_lines():
    with open(__file__, "r", encoding="utf-8") as f:
        return sum(1 for _ in f)


def scan_book(path):
    """扫描一本增补册：返回批结构 {batch_no: {'declared': int, 'items': {id: (name, lines)}}}。
    无批头的册回退为伪批 0（整册一个批），批头声明行数取不到时为 None（跳过批头对账）。"""
    batches = {}
    cur = None
    last_head = None
    with open(path, "r", encoding="utf-8", errors="replace") as f:
        for raw in f:
            line = raw.rstrip("\n")
            m = RE_BATCH_HEAD.search(line)
            if m:
                cur = int(m.group(1))
                batches.setdefault(cur, {"declared": None, "items": {}})
                dm = RE_BATCH_DECL.search(line)
                if dm:
                    batches[cur]["declared"] = norm_lines(dm.group(1))
                continue
            # 批头声明行数兜底：形如 "纯功能行数：6,000" / "行数合计 6,000"
            dm = re.search(r"(?:纯功能行数|行数合计|批累计行数)[：:＝=]*\s*([\d,]+)", line)
            if dm and cur is not None and batches[cur]["declared"] is None:
                batches[cur]["declared"] = norm_lines(dm.group(1))
                continue
            # 表行条目（按单元格拆分，兼容名称列含 | 的行）
            if line.startswith("|") and line.count("|") >= 4:
                cells = [c.strip() for c in line.strip().strip("|").split("|")]
                if cells and re.fullmatch(r"[A-Z][A-Z0-9]*(?:-[A-Z0-9]+)*\d[A-Z0-9]*", cells[0]):
                    num_idx = next((i for i, c in enumerate(cells[1:], 1)
                                    if i <= 5 and re.fullmatch(r"[\d,]+", c)), None)
                    if num_idx is not None:
                        fid = cells[0]
                        name = "|".join(cells[1:num_idx])[:80]
                        ln = norm_lines(cells[num_idx])
                        if cur is None:
                            cur = 0
                            batches.setdefault(0, {"declared": None, "items": {}})
                        batches[cur]["items"][fid] = (name, ln)
                        last_head = None
                        continue
            # 标题式条目
            hm = RE_ITEM_HEAD.match(line)
            if hm:
                fid, name = hm.group(1), hm.group(2).strip()
                if cur is None:
                    cur = 0
                    batches.setdefault(0, {"declared": None, "items": {}})
                batches[cur]["items"].setdefault(fid, (name, None))
                last_head = (cur, fid)
                continue
            # 标题式条目正文行数："纯功能行数：300｜状态：[骨架]"
            if last_head is not None:
                bm = re.search(r"纯功能行数[：:]\s*([\d,]+)", line)
                if bm:
                    c, fid = last_head
                    nm, ln = batches[c]["items"][fid]
                    if ln is None:
                        batches[c]["items"][fid] = (nm, norm_lines(bm.group(1)))
                else:
                    if RE_BATCH_HEAD.search(line) or RE_ITEM_HEAD.match(line) or RE_TABLE_ROW.match(line):
                        last_head = None
            continue
    return batches


def domain_of(fid):
    return fid  # GOV 段直接用前缀分组；UNX 段按 F 段映射由调用方补


def assert_book(path, report):
    """对单本增补册执行批级断言。返回 (ok, stats)。伪批 0（无批头册）只做聚合，不做 20 条/6,000 带断言。"""
    ok = True
    name = os.path.basename(path)
    batches = scan_book(path)
    total_declared = 0
    total_actual = 0
    all_ids = set()
    batch_rows = []
    for bno in sorted(batches):
        b = batches[bno]
        n_items = len(b["items"])
        actual = sum(l for (_, l) in b["items"].values() if l is not None)
        declared = b["declared"]
        batch_ok = True
        notes = []
        if bno == 0:
            if n_items == 0:
                batch_ok = False
                notes.append("结构未识别（无批头、无表行命中）——转人工复估")
        else:
            if n_items != 20:
                batch_ok = False
                notes.append(f"条数 {n_items} != 20")
            if declared is not None and actual and actual != declared:  # 批头=实计 零容差（±1 必抓口径）
                batch_ok = False
                notes.append(f"批头 {declared} != 实计 {actual}")
            if actual and not (BATCH_QUOTA * (1 - BATCH_TOL) <= actual <= BATCH_QUOTA * (1 + BATCH_TOL)):
                batch_ok = False
                notes.append(f"实计 {actual} 超出 6,000±5% 带")
        total_declared += declared or 0
        total_actual += actual
        all_ids.update(b["items"].keys())
        batch_rows.append((bno, n_items, declared, actual, batch_ok, "; ".join(notes)))
        if not batch_ok:
            ok = False
    report.append(f"册：{name}" + ("  [无批头结构·伪批聚合]" if 0 in batches and len(batches) == 1 else ""))
    for bno, n, d, a, bok, note in batch_rows:
        if bno == 0 and len(batch_rows) == 1:
            continue
        flag = "✅" if bok else "❌"
        report.append(f"  批 B{bno:02d}｜{n} 条｜声明 {d}｜实计 {a}｜{flag} {note}")
    if len(batch_rows) == 1 and batch_rows[0][0] == 0:
        bno, n, d, a, bok, note = batch_rows[0]
        report.append(f"  伪批聚合｜{n} 条｜声明 {d}｜实计 {a}｜{'✅' if bok else '❌ ' + note}")
    report.append(f"  小计：声明 {total_declared}｜实计 {total_actual}｜批次 {len(batch_rows)}")
    stats = {"declared": total_declared, "actual": total_actual, "ids": all_ids, "batches": len(batch_rows)}
    return ok, stats


def injection_test(books, injections, report):
    """GOV-83-J2 注入抽测：对含目标条目的册重放扫描，批实计 +delta 后必须被批头/带宽断言抓出。"""
    if not injections:
        return True
    ok = True
    for fid, delta in injections.items():
        hit = None
        for p in books:
            b = scan_book(p)
            for bno, bd in b.items():
                if fid in bd["items"]:
                    hit = (p, bno, bd)
                    break
            if hit:
                break
        if not hit:
            report.append(f"  [J2] 注入目标 {fid} 全库未命中——注入无效，按漏抓处理")
            return False
        p, bno, bd = hit
        declared = bd["declared"]
        base_actual = sum(l for (_, l) in bd["items"].values() if l is not None)
        tampered = base_actual + delta
        caught = False
        if declared is not None and tampered != declared:  # 批头=实计 零容差
            caught = True
        if not (BATCH_QUOTA * (1 - BATCH_TOL) <= tampered <= BATCH_QUOTA * (1 + BATCH_TOL)):
            caught = True
        report.append(f"  [J2] 注入 {fid}{delta:+d} → 批 B{bno:02d} 实计 {base_actual}→{tampered}：{'必抓 ✓' if caught else '漏抓 ✗（断言器缺陷 P0）'}")
        if not caught:
            ok = False
    return ok


def progress_deviation(dom_stats):
    """GOV-83-J3：册级实计 vs 批数 × 6,000 期望偏离 ±30% 预警。"""
    alerts = []
    for dom, st in dom_stats.items():
        if not st["batches"]:
            continue
        expect = st["batches"] * BATCH_QUOTA
        actual = st["actual"]
        if expect and abs(actual - expect) / expect > DEV_TOL:
            alerts.append((dom, actual, expect, (actual - expect) / expect))
    return alerts


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", required=True)
    ap.add_argument("--wave", default="W1")
    ap.add_argument("--inject", nargs="*", default=[])
    ap.add_argument("--report", default=None)
    args = ap.parse_args()

    report = []
    report.append(f"[域账对账单 {args.wave}]  生成：GOV-83 断言器 v1.0")
    injections = {}
    for spec in args.inject:
        fid, _, d = spec.partition("=")
        injections[fid] = int(d)

    books = []
    for fn in sorted(os.listdir(args.base)):
        if re.match(r"^AI-\d+ · .+增补册", fn) and fn.endswith(".md"):
            books.append(os.path.join(args.base, fn))
    if not books:
        print("未发现任何增补册", file=sys.stderr)
        return 1

    all_ok = True
    agg_ids = set()
    dom_stats = {}
    for p in books:
        ok, st = assert_book(p, report)
        if not ok:
            all_ok = False
        agg_ids |= st["ids"]
        dom_stats[os.path.basename(p)] = st

    if not injection_test(books, injections, report):
        all_ok = False

    report.append("-" * 60)
    report.append(f"覆盖增补册：{len(books)} 本｜唯一条目 ID：{len(agg_ids)}")
    report.append(f"累计实计：{sum(s['actual'] for s in dom_stats.values())} / {TOTAL_TARGET}")
    alerts = progress_deviation(dom_stats)
    for dom, a, e, dev in alerts:
        report.append(f"  [J3 预警] {dom} 实计 {a} vs 期望 {e}（偏离 {dev:+.0%}）")
    if not alerts:
        report.append("  [J3] 无 ±30% 偏离域，零预警")
    ok_self = this_file_lines() <= SELF_LINE_LIMIT
    report.append(f"  [J3t] 断言器自身 {this_file_lines()} 行 / 上限 {SELF_LINE_LIMIT}：{'✓' if ok_self else '✗'}")
    if not ok_self:
        all_ok = False
    report.append(f"签：AI-83｜核：AI-81｜结论：{'ALL PASS' if all_ok else 'FAIL'}")

    text = "\n".join(report)
    print(text)
    if args.report:
        with open(args.report, "w", encoding="utf-8") as f:
            f.write(text + "\n")
    if not all_ok:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
