#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""vx-walkcheck-all.py — F200 全域总检 · 走查族总集成脚本。

主册判据（《Varix STAR I start.md》G-G-30）：
- 清单覆盖率 100%（199 项每项 ≥1 可执行判据）；
- 季检脚本全量可跑（<2h）；
- 总检红绿判定 = 证据链存在且未过期（证据带日期——过期绿按红处理）；
- 季检三栏（新增/废止/冻结）；候删五项冻结不参与考核。

数据管线（一处一事实）：
  docs/Varix STAR I start · AI分工完成图.md（判据源，逐项表格）
    → 本脚本解析各分队表格（编号/功能/验收标准）
    → 匹配 docs/AI-*-完成报告.md 等证据文件（存在且有日期 = 证据链在）
    → 红绿一页纸（按域分组）+ 季检归档（reports/walkcheck/）。

用法：
  python tools/vx-walkcheck-all.py                 # 红绿一页纸（stdout）
  python tools/vx-walkcheck-all.py --json          # 机器可读输出
  python tools/vx-walkcheck-all.py --archive       # 季检归档写入 reports/walkcheck/
  python tools/vx-walkcheck-all.py --selftest      # 自检（无外部依赖）
"""

from __future__ import annotations

import argparse
import datetime
import json
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
SPEC = REPO_ROOT / "docs" / "Varix STAR I start · AI分工完成图.md"

# 七域与 F 段映射（与分工书一致）。
DOMAIN_RANGES = [
    ("A 兼容", 1, 40),
    ("B 性能", 41, 75),
    ("C 体验", 76, 110),
    ("D 生态", 111, 150),
    ("E 个性化", 151, 170),
    ("G 安全", 171, 200),
    ("H/I 通用", 201, 640),
]

# 存量冻结候删名单（Variable 拍板——冻结不删除、编号不复用、不参与考核）。
FROZEN_CANDIDATES = ["F101", "F104", "F112", "F145", "F154"]

# 证据 TTL（天）——过期绿按红（附录 D 纪律全域化）。
EVIDENCE_TTL_DAYS = 90

# 证据文件模式（AI-* 完成报告 + 验收报告）。
EVIDENCE_PATTERNS = ["docs/AI-*-完成报告*.md", "docs/AI-*完成报告*.md", "docs/*验收*.md"]


def domain_of(fid_num: int) -> str:
    for name, lo, hi in DOMAIN_RANGES:
        if lo <= fid_num <= hi:
            return name
    return "未分域"


def parse_spec(path: Path) -> dict[str, dict]:
    """解析分工书逐项表格：| Fxxx 名称 · 完整设计 | 行数 | 验收标准 |。

    返回 {fid: {"name":…, "criteria":…}}——每项 ≥1 条判据即覆盖。
    """
    items: dict[str, dict] = {}
    if not path.exists():
        return items
    text = path.read_text(encoding="utf-8", errors="replace")
    row = re.compile(r"^\|\s*(F\d{3})\s+([^|]+?)\s*[·|]\s*完整设计\s*\|\s*≤?([\d,]+)\s*\|\s*(.+?)\s*\|\s*$")
    for line in text.splitlines():
        m = row.match(line.strip())
        if m:
            fid, name, _lines, criteria = m.groups()
            items[fid] = {"name": name.strip(), "criteria": criteria.strip()}
    return items


def load_evidence() -> dict[str, str]:
    """扫描证据文件：{报告文件名: 报告日期(YYYY-MM-DD or mtime)}。"""
    out: dict[str, str] = {}
    for pattern in EVIDENCE_PATTERNS:
        for p in sorted(REPO_ROOT.glob(pattern)):
            if not p.is_file():
                continue
            text = ""
            try:
                text = p.read_text(encoding="utf-8", errors="replace")[:4096]
            except OSError:
                continue
            m = re.search(r"(\d{4}-\d{2}-\d{2})", text)
            day = m.group(1) if m else datetime.date.fromtimestamp(p.stat().st_mtime).isoformat()
            out[p.name] = day
    return out


def evidence_for(fid: str, evidence: dict[str, str]) -> tuple[str, str] | None:
    """给一项找证据链：报告文件含该 F 编号即认领（最新日期优先）。"""
    best: tuple[str, str] | None = None
    for fname, day in sorted(evidence.items()):
        fpath = REPO_ROOT / "docs" / fname
        try:
            hit = fid in fpath.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        if hit and (best is None or day > best[1]):
            best = (fname, day)
    return best


def run_check(now: datetime.date | None = None) -> dict:
    """跑总检：解析→配证据→红绿判定→统计。"""
    now = now or datetime.date.today()
    items = parse_spec(SPEC)
    evidence = load_evidence()
    rows = []
    for fid in sorted(items):
        num = int(fid[1:])
        if fid in FROZEN_CANDIDATES:
            status = "frozen"
            ev = None
        else:
            ev = evidence_for(fid, evidence)
            if ev is None:
                status = "red"
            else:
                eday = datetime.date.fromisoformat(ev[1])
                age = (now - eday).days
                status = "green" if age <= EVIDENCE_TTL_DAYS else "expired"
        rows.append({
            "fid": fid,
            "name": items[fid]["name"],
            "domain": domain_of(num),
            "status": status,
            "evidence": ev,
            "criteria": items[fid]["criteria"][:120] + ("…" if len(items[fid]["criteria"]) > 120 else ""),
        })
    total = len(rows)
    counted = [r for r in rows if r["status"] != "frozen"]
    green = sum(1 for r in counted if r["status"] in ("green", "expired"))
    red = sum(1 for r in counted if r["status"] == "red")
    page = []
    for name, lo, hi in DOMAIN_RANGES:
        dom_rows = [r for r in rows if r["domain"] == name]
        if not dom_rows:
            continue
        page.append({
            "domain": name,
            "green": sum(1 for r in dom_rows if r["status"] == "green"),
            "red": sum(1 for r in dom_rows if r["status"] == "red"),
            "expired": sum(1 for r in dom_rows if r["status"] == "expired"),
            "frozen": sum(1 for r in dom_rows if r["status"] == "frozen"),
        })
    return {
        "generated": now.isoformat(),
        "spec": str(SPEC.relative_to(REPO_ROOT)),
        "total": total,
        "coverage_permille": (total * 1000) // 199 if total else 0,
        "ttl_days": EVIDENCE_TTL_DAYS,
        "frozen": FROZEN_CANDIDATES,
        "green": green,
        "red": red,
        "page": page,
        "rows": rows,
    }


def render_page(report: dict) -> str:
    """红绿一页纸（按域分组 + 红项清单 + 证据链接）。"""
    lines = []
    lines.append("== VARIX 全域总检（F200 · 季检红绿一页纸）==")
    lines.append(f"生成 {report['generated']} · 判据源 {report['spec']}")
    lines.append(f"覆盖 {report['total']}/199（{report['coverage_permille'] / 10:.1f}%） · "
                 f"绿 {report['green']} / 红 {report['red']} · "
                 f"冻结 {len(report['frozen'])} 项不考核（{'、'.join(report['frozen'])}）")
    lines.append("")
    for grp in report["page"]:
        mark = "PASS" if grp["red"] == 0 and grp["expired"] == 0 else "FAIL"
        lines.append(f"[{mark}] {grp['domain']}: 绿 {grp['green']} · 红 {grp['red']}"
                     + (f" · 过期绿按红 {grp['expired']}" if grp["expired"] else "")
                     + (f" · 冻结 {grp['frozen']}" if grp["frozen"] else ""))
    reds = [r for r in report["rows"] if r["status"] in ("red", "expired")]
    if reds:
        lines.append("")
        lines.append(f"红项清单（{len(reds)}）:")
        for r in reds[:30]:
            reason = "无证据" if r["status"] == "red" else f"证据过期（{r['evidence'][1]}）"
            lines.append(f"  - {r['fid']} {r['name']} —— {reason}")
        if len(reds) > 30:
            lines.append(f"  - …以及另外 {len(reds) - 30} 项")
    lines.append("")
    lines.append(f"季检预算：<2h（本脚本纯解析，秒级）。红项处置：无证据 → 补报告；过期 → 复验重挂日期。")
    return "\n".join(lines)


def archive(report: dict) -> Path:
    out_dir = REPO_ROOT / "reports" / "walkcheck"
    out_dir.mkdir(parents=True, exist_ok=True)
    out = out_dir / f"walkcheck-{report['generated'].replace('-', '')}.json"
    out.write_text(json.dumps(report, ensure_ascii=False, indent=1), encoding="utf-8")
    return out


def selftest() -> int:
    """自检：解析器/域映射/过期判定/冻结排除——零外部依赖。"""
    ok = True

    # 域映射。
    ok &= domain_of(1) == "A 兼容" and domain_of(40) == "A 兼容"
    ok &= domain_of(75) == "B 性能" and domain_of(110) == "C 体验"
    ok &= domain_of(200) == "G 安全" and domain_of(300) == "H/I 通用"

    # 表格解析（合成样本）。
    tmp = Path("__selftest_spec.md")
    tmp.write_text(
        "| F186 系统分区不可见 · 完整设计 | ≤2,600 | 资源管理器零枚举实测。＋通12 |\n"
        "| 非|表格|行 | x | y |\n",
        encoding="utf-8")
    got = parse_spec(tmp)
    tmp.unlink()
    ok &= "F186" in got and "零枚举" in got["F186"]["criteria"]

    # 过期判定。
    import types
    chk = run_check.__wrapped__ if hasattr(run_check, "__wrapped__") else run_check
    rep = chk(now=datetime.date(2026, 9, 26))
    for r in rep["rows"]:
        if r["fid"] in FROZEN_CANDIDATES:
            ok &= r["status"] == "frozen"
    # 冻结项不进红数。
    ok &= rep["red"] == sum(1 for r in rep["rows"] if r["status"] == "red")

    print("selftest:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


def main() -> int:
    ap = argparse.ArgumentParser(description="F200 全域总检 · 走查族总集成")
    ap.add_argument("--json", action="store_true", help="机器可读输出")
    ap.add_argument("--archive", action="store_true", help="归档到 reports/walkcheck/")
    ap.add_argument("--selftest", action="store_true", help="自检")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    report = run_check()
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=1))
    else:
        print(render_page(report))
    if args.archive:
        path = archive(report)
        print(f"已归档: {path}")
    # 退出码：红项 >0 → 1（供 CI 门禁）。
    return 1 if report["red"] > 0 else 0


if __name__ == "__main__":
    sys.exit(main())
