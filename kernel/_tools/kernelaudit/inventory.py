# -*- coding: utf-8 -*-
"""规模清单：按域统计文件数、行数、超标文件数、判据入口数、测试数。

只做计数，不做判断。判断留给 claims / hazards / registry 三个模块，
这样「规模」与「质量」在报告里是两条独立的证据线，不会互相解释掉。
"""

from __future__ import annotations

import re
from collections import defaultdict
from dataclasses import dataclass, field
from typing import Dict, List, Tuple

from . import sources

_BIG = 1000       # 单文件代码行上限（超此值列为 modularization 待办）
_HUGE = 2000      # 明显违反模块化的阈值
_ENTRY_RX = re.compile(r"pub\s+fn\s+run_\w*_checks")


@dataclass
class DomainStat:
    name: str
    files: int = 0
    lines: int = 0
    big: int = 0
    huge: int = 0
    entries: int = 0
    tests: int = 0

    def as_row(self) -> Tuple[str, ...]:
        return (
            self.name,
            str(self.files),
            f"{self.lines:,}",
            str(self.big),
            str(self.huge),
            str(self.entries),
            str(self.tests),
        )


@dataclass
class Inventory:
    domains: Dict[str, DomainStat] = field(default_factory=dict)

    @property
    def total(self) -> DomainStat:
        t = DomainStat("TOTAL")
        for d in self.domains.values():
            t.files += d.files
            t.lines += d.lines
            t.big += d.big
            t.huge += d.huge
            t.entries += d.entries
            t.tests += d.tests
        return t

    def ranked(self) -> List[DomainStat]:
        return sorted(self.domains.values(), key=lambda d: -d.lines)

def largest_files(root: str, n: int = 15) -> List[Tuple[int, str]]:
    """全树最大的 n 个文件（按行数）。独立于 Inventory，避免重复解析。"""
    import os

    rows: List[Tuple[int, str]] = []
    for dp, dn, fn in os.walk(root):
        for f in fn:
            if not f.endswith(".rs"):
                continue
            p = os.path.join(dp, f)
            try:
                b = open(p, "rb").read()
            except OSError:
                continue
            rel = os.path.relpath(p, root).replace(os.sep, "/")
            rows.append((b.count(b"\n") + 1, rel))
    rows.sort(reverse=True)
    return rows[:n]


def audit(root: str) -> Inventory:
    inv = Inventory()
    for sf in sources.iter_sources(root):
        d = inv.domains.setdefault(sf.domain, DomainStat(sf.domain))
        code_lines = sf.code.count("\n") + 1
        d.files += 1
        d.lines += code_lines
        if code_lines > _BIG:
            d.big += 1
        if code_lines > _HUGE:
            d.huge += 1
        d.entries += len(sources.count_matches(sf.code, _ENTRY_RX.pattern))
        d.tests += sf.text.count("#[test]")
    return inv
