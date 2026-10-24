# -*- coding: utf-8 -*-
"""渲染层：把三类审计结果压成 Markdown 表格与 JSON。

报告只陈述**可复现的事实**（数字、文件、行号），不写结论性形容词。
结论由人写——机器负责让结论无法被含糊过去。
"""

from __future__ import annotations

import json
from typing import Dict, List

from . import claims as claims_mod
from . import hazards as hazards_mod
from . import inventory as inv_mod
from . import registry as reg_mod


def _table(header: List[str], rows: List[List[str]]) -> str:
    out = ["| " + " | ".join(header) + " |", "|" + "|".join(["---"] * len(header)) + "|"]
    for r in rows:
        out.append("| " + " | ".join(r) + " |")
    return "\n".join(out)


def render_inventory(inv: inv_mod.Inventory, top_files: List[tuple], top_n: int = 20) -> str:
    rows = [list(d.as_row()) for d in inv.ranked()[:top_n]]
    total = inv.total
    rows.append(list(total.as_row()))
    parts = [
        "## 1 · 规模清单（按域）",
        "",
        _table(["域", "文件", "代码行", ">1k 行", ">2k 行", "判据入口", "测试"], rows),
        "",
        f"全树 **{total.files}** 个 `.rs`，**{total.lines:,}** 代码行（已剥 `#[cfg(test)]`），"
        f"**{len(inv.domains)}** 个域，**{total.entries}** 个 `run_*_checks` 入口，"
        f"**{total.tests}** 个 `#[test]`。",
        "",
        "### 最大的文件（模块化待办）",
        "",
        _table(["行数", "文件"], [[f"{n:,}", p] for n, p in top_files[:12]]),
        "",
    ]
    return "\n".join(parts)


def render_hazards(rep: hazards_mod.HazardReport, top_n: int = 12) -> str:
    dom = rep.by_domain()
    rows = [
        [d, str(v["files"]), str(v["kernel"]), str(v["judge"]), str(v["test"])]
        for d, v in sorted(dom.items(), key=lambda kv: -kv[1]["kernel"])[:top_n]
    ]
    worst = rep.worst(top_n)
    detail = [[f.rel, str(f.kernel), str(f.judge), "; ".join(f.samples[:2])[:120]] for f in worst if f.kernel or f.judge]
    parts = [
        "## 2 · panic 面（裸目标无 unwind，panic = 停机）",
        "",
        _table(["域", "文件", "内核路径", "判据层", "测试层"], rows),
        "",
        f"- 内核路径触发点：**{rep.kernel}**",
        f"- 判据层触发点：**{rep.judge}**",
        f"- 测试层触发点：**{rep.test}**（豁免，宿主 std）",
        f"- 内核路径零触发点的文件：**{rep.clean_kernel_files()}/{len(rep.files)}**",
        "",
    ]
    if detail:
        parts += ["### 命中最多的文件", "", _table(["文件", "内核", "判据", "样例"], detail), ""]
    return "\n".join(parts)


def render_claims(bad: List[claims_mod.ClaimResult], scanned: int, top_n: int = 20) -> str:
    rows = [
        [r.rel, r.claim.kind, str(r.count), r.note]
        for r in bad[:top_n]
    ]
    kinds = claims_mod.summarize(bad)
    parts = [
        "## 3 · 文档承诺 vs 实现（把注释变成断言）",
        "",
        f"扫描带承诺声明的文件 **{scanned}** 个，未兑现 **{len(bad)}** 条。",
        "",
        _table(["承诺类型", "未兑现文件数"], [[k, str(v)] for k, v in sorted(kinds.items())]),
        "",
    ]
    if rows:
        parts += [_table(["文件", "承诺", "命中", "处置方向"], rows), ""]
    return "\n".join(parts)


def render_registry(rep: reg_mod.RegistryReport) -> str:
    parts = [
        "## 4 · 自检注册闭环",
        "",
        f"- `lib.rs` 顶层模块：**{len(rep.modules)}**",
        f"- 定义自检入口的域：**{len(rep.judged_domains)}**（其余 {len(rep.unjuried_domains)} 个是基础设施模块，本就无入口）",
        f"- 注册表容量声明：**{rep.declared_capacity}**，表内登记：**{rep.table_size}**，表外单独 register：**{len(rep.out_of_table)}**",
        f"- 已覆盖域：**{len(rep.covered_domains)}**",
        f"- **有入口却没进表（孤儿）：{len(rep.orphan_domains)}**",
        f"- 重复登记：**{len(rep.duplicates)}**",
        "",
    ]
    if rep.error:
        parts.insert(2, f"> 读取失败：{rep.error}")
        parts.append("")
        return "\n".join(parts)
    if rep.orphan_domains:
        parts += ["### 有自检入口但未进注册表的域（其判据永不执行）", "", "`" + "`, `".join(rep.orphan_domains) + "`", ""]
    if rep.duplicates:
        parts += ["### 重复登记", "", "`" + "`, `".join(rep.duplicates) + "`", ""]
    if rep.table_size != rep.declared_capacity:
        parts += [
            f"> 容量声明 {rep.declared_capacity} 与实登记 {rep.table_size} 不一致："
            "数组长度由字面量写死，多一条少一条都会编译失败，此二者必须相等。",
            "",
        ]
    return "\n".join(parts)


def render_all(
    inv: inv_mod.Inventory,
    haz: hazards_mod.HazardReport,
    bad_claims: List[claims_mod.ClaimResult],
    scanned: int,
    reg: reg_mod.RegistryReport,
    top_files: List[tuple],
) -> str:
    return "\n".join(
        [
            "# Varix 内核 · 全域静态审计",
            "",
            "> 本文件由 `python _tools/kernelaudit` 生成，数字可用同一命令复现。",
            "",
            render_inventory(inv, top_files),
            render_hazards(haz),
            render_claims(bad_claims, scanned),
            render_registry(reg),
        ]
    )


def to_json(
    inv: inv_mod.Inventory,
    haz: hazards_mod.HazardReport,
    bad_claims: List[claims_mod.ClaimResult],
    scanned: int,
    reg: reg_mod.RegistryReport,
) -> str:
    payload: Dict[str, object] = {
        "inventory": {
            "domains": len(inv.domains),
            "files": inv.total.files,
            "lines": inv.total.lines,
            "entries": inv.total.entries,
            "tests": inv.total.tests,
            "per_domain": {d.name: d.__dict__ for d in inv.ranked()},
        },
        "hazards": {
            "kernel": haz.kernel,
            "judge": haz.judge,
            "test": haz.test,
            "worst": [
                {"file": f.rel, "kernel": f.kernel, "judge": f.judge, "samples": f.samples}
                for f in haz.worst(30)
            ],
        },
        "claims": {
            "scanned_files": scanned,
            "violations": [
                {
                    "file": r.rel,
                    "kind": r.claim.kind,
                    "count": r.count,
                    "hits": [{"line": h.line, "snippet": h.snippet} for h in r.hits[:20]],
                    "note": r.note,
                }
                for r in bad_claims
            ],
        },
        "registry": {
            "modules": reg.modules,
            "capacity": reg.declared_capacity,
            "entries": reg.table_size,
            "orphans": reg.orphan_domains,
            "duplicates": reg.duplicates,
            "error": reg.error,
        },
    }
    return json.dumps(payload, ensure_ascii=False, indent=2)
