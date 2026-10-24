# -*- coding: utf-8 -*-
"""自检注册表闭环对账。

内核的验收结构是「每个域写一个 `run_*_checks() -> CheckSet`，由
`robust.rs::run_kernel_checkup()` 的函数指针表统一调用」。这张表是**判据的唯一
入口**：没进表的域，判据写得再全也不会被执行——编译绿、测试也绿，而判据等于不存在。

历史上真实发生过：unxstar（69 子域）与 cgpu（52 条）两册合计 240 万行判据
目录外零引用，永不执行，直到 2026-10-19 才补进表里。本模块把这件事变成一条
可复跑的断言，防止下一个域重演。

对账四问：
  1. `lib.rs` 声明了哪些顶层模块？
  2. 注册表登记了哪些入口（函数名 + 归属 crate 路径）？
  3. 哪些顶层模块**一条入口都没进表**（孤儿域）？
  4. 表里有没有重复登记（同一函数登记两次 ⇒ 检查数虚高）？
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Dict, List, Optional, Sequence, Set, Tuple

LIB = "lib.rs"
ROBUST = "robust.rs"

_MOD_DECL_RX = re.compile(r"^\s*pub\s+mod\s+([A-Za-z_][A-Za-z0-9_]*)", re.M)
_TABLE_DECL_RX = re.compile(r"let\s+domains\s*:\s*\[fn\(\)\s*->\s*CheckSet;\s*(\d+)\]\s*=\s*\[")
_ENTRY_RX = re.compile(r"crate::([A-Za-z_][A-Za-z0-9_:]*)")


@dataclass
class RegistryReport:
    modules: List[str] = field(default_factory=list)
    entries: List[str] = field(default_factory=list)
    out_of_table: List[str] = field(default_factory=list)
    declared_capacity: int = 0
    duplicates: List[str] = field(default_factory=list)
    orphan_domains: List[str] = field(default_factory=list)
    covered_domains: List[str] = field(default_factory=list)
    judged_domains: List[str] = field(default_factory=list)
    unjuried_domains: List[str] = field(default_factory=list)
    error: str = ""

    @property
    def table_size(self) -> int:
        return len(self.entries)


def _read(root: str, rel: str) -> str:
    import os

    path = os.path.join(root, rel.replace("/", os.sep))
    try:
        return open(path, encoding="utf-8", errors="replace").read()
    except OSError:
        return ""


def _table_body(text: str) -> Tuple[int, str]:
    m = _TABLE_DECL_RX.search(text)
    if not m:
        return 0, ""
    cap = int(m.group(1))
    start = text.find("[", m.end() - 1)
    if start < 0:
        return cap, ""
    depth = 0
    for i in range(start, len(text)):
        if text[i] == "[":
            depth += 1
        elif text[i] == "]":
            depth -= 1
            if depth == 0:
                return cap, text[start : i + 1]
    return cap, text[start:]


# `pub fn run_xxx_checks(...) -> CheckSet` —— **真正能进注册表**的入口形态。
# 必须卡返回类型：仓里有同名但返回 `(usize, usize)` 的旧式自检
# （如 cpu/cpuinfo.rs 的 `run_interrupt_checks`），只按名字匹配会把它们
# 误算成「有入口却未进表」的孤儿。
_ENTRY_DEF_RX = re.compile(
    r"pub\s+fn\s+run_[A-Za-z0-9_]*checks\s*\([^)]*\)\s*->\s*CheckSet\b",
    re.S,
)


def domains_with_entries(root: str) -> Set[str]:
    """返回「至少定义了一个 `-> CheckSet` 自检入口」的顶层域名集合。"""
    from . import sources as src

    out: Set[str] = set()
    for sf in src.iter_sources(root):
        if _ENTRY_DEF_RX.search(sf.code):
            out.add(sf.domain)
    return out


def _function_body(text: str, name: str) -> str:
    """取 `fn <name>(...) { ... }` 的函数体（已剥注释的文本上做括号匹配）。"""
    m = re.search(r"fn\s+" + re.escape(name) + r"\s*\(", text)
    if not m:
        return ""
    brace = text.find("{", m.end())
    if brace < 0:
        return ""
    depth = 0
    for i in range(brace, len(text)):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                return text[brace : i + 1]
    return text[brace:]


def audit(root: str) -> RegistryReport:
    rep = RegistryReport()

    lib = _read(root, LIB)
    if not lib:
        rep.error = f"读不到 {LIB}"
        return rep
    # 去掉注释再抽模块名，避免被文档里的示例行误导
    from .sources import mask_rust

    rep.modules = _MOD_DECL_RX.findall(mask_rust(lib))

    robust = _read(root, ROBUST)
    if not robust:
        rep.error = f"读不到 {ROBUST}"
        return rep
    masked = mask_rust(robust)
    cap, body = _table_body(masked)
    rep.declared_capacity = cap
    rep.entries = _ENTRY_RX.findall(body)

    # 表外注册：`checkup.register(crate::xxx::run_xxx_checks())`。
    # 历史原因，unxstar / cgpu 两册是在循环之后单独 register 的——只看表体
    # 会把它们误报成孤儿域（240 万行判据「永不执行」的误判方向恰好相反，
    # 但同样是错的）。所以以 run_kernel_checkup 整个函数体为准做覆盖判定。
    fn_body = _function_body(masked, "run_kernel_checkup")
    all_refs = _ENTRY_RX.findall(fn_body) if fn_body else list(rep.entries)
    rep.out_of_table = [e for e in all_refs if e not in set(rep.entries)]

    seen: Set[str] = set()
    for e in rep.entries:
        if e in seen:
            rep.duplicates.append(e)
        seen.add(e)

    covered: Set[str] = set()
    for e in all_refs:
        covered.add(e.split(":")[0])
    rep.covered_domains = sorted(covered)

    # 只有「真的定义了自检入口」的域才配谈孤儿：`acpi` / `console` / `cpu`
    # 这类基础设施模块本来就没有 `run_*_checks`，把它们算成「判据永不执行」
    # 是把噪音当缺陷。判据是「有入口却没进表」。
    defining = domains_with_entries(root)
    rep.judged_domains = sorted(defining)
    rep.orphan_domains = sorted(m for m in rep.modules if m in defining and m not in covered)
    rep.unjuried_domains = sorted(m for m in rep.modules if m not in defining)
    return rep


def coverage_ratio(rep: RegistryReport) -> float:
    if not rep.modules:
        return 0.0
    return len(rep.covered_domains) / float(len(rep.modules))
