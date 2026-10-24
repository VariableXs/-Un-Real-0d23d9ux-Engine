# -*- coding: utf-8 -*-
"""panic 面与静默面普查（条目粒度）。

裸目标（`x86_64-unknown-none`）没有 unwind：任何 `panic!` 都等于停机。所以
「内核路径上有多少条 panic 触发点」是第一质量指标，比警告数重要得多。

**为什么必须做到条目粒度**：本仓的文件名不是分类依据。`unx097_enableseq.rs`
同一个文件里既有实现（`EnableBoard::program_entry`），也有判据
（`j1b_unready_intercept_10`）。按文件分类会把 25 条只在测试里跑的 unwrap
算进内核路径，得出 3000+ 的吓人数字；按条目分类才看得出真相：实现层其实很干净，
脏的是判据函数。

分类规则（与仓内命名惯例一一对应，可核）：
  判据   文件名以 `_checks.rs` / `_test.rs` 结尾，或位于 `tests/`
         函数名以 `j<数字>` 开头，或含 `judge` / `matrix` / `selftest` / `export_`
  测试   `#[cfg(test)]` 区内（宿主 std，豁免）
  内核   其余一切
"""

from __future__ import annotations

import re
from collections import defaultdict
from dataclasses import dataclass, field
from typing import Dict, List, Tuple

from . import sources
from .sources import SourceFile

_PANIC_RX = re.compile(r"\.unwrap\s*\(|\.expect\s*\(|\bpanic!\s*\(|\btodo!\s*\(|\bunimplemented!\s*\(")
_UNSAFE_RX = re.compile(r"\bunsafe\s*\{")
_LET_DISCARD_RX = re.compile(r"\blet\s+_\s*=")
_SWALLOW_RX = re.compile(r"\.unwrap_or_default\s*\(")
_ALLOW_RX = re.compile(r"#\[allow\s*\(")

_FN_NAME_RX = re.compile(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)")
_JUDGE_NAME_RX = re.compile(r"^(j\d|judge|matrix|selftest)|(?:^|_)(?:judge|matrix|selftest|export)_")

CAT_KERNEL = "kernel"
CAT_JUDGE = "judge"
CAT_TEST = "test"


@dataclass
class HazardFile:
    rel: str
    domain: str
    kernel: int = 0
    judge: int = 0
    test: int = 0
    unsafe_blocks: int = 0
    let_discard: int = 0
    swallow: int = 0
    allows: int = 0
    samples: List[str] = field(default_factory=list)

    @property
    def total(self) -> int:
        return self.kernel + self.judge + self.test


def _is_judge_name(name: str) -> bool:
    return bool(_JUDGE_NAME_RX.search(name))


def _classify_items(sf: SourceFile) -> List[Tuple[int, int, str]]:
    """把文件切成 (start, end, 分类) 三元组。"""
    out: List[Tuple[int, int, str]] = []
    items = sources.top_level_items(sf.masked)
    if not items:
        out.append((0, len(sf.masked), CAT_JUDGE if sf.is_check_layer else CAT_KERNEL))
        return out

    cursor = 0
    for start, end, kind in items:
        if start > cursor:
            # 条目之间的空白/属性区，归到「按文件定」的默认类
            out.append((cursor, start, CAT_JUDGE if sf.is_check_layer else CAT_KERNEL))
        sig = sf.masked[start : min(end, start + 400)]
        m = _FN_NAME_RX.search(sig)
        name = m.group(1) if m else ""
        if sf.is_check_layer or _is_judge_name(name):
            cat = CAT_JUDGE
        else:
            cat = CAT_KERNEL
        out.append((start, end, cat))
        cursor = end
    if cursor < len(sf.masked):
        out.append((cursor, len(sf.masked), CAT_JUDGE if sf.is_check_layer else CAT_KERNEL))
    return out


def _scan(sf: SourceFile) -> HazardFile:
    hf = HazardFile(rel=sf.rel, domain=sf.domain)

    for start, end, cat in _classify_items(sf):
        chunk = sf.code[start:end]          # 生产区遮罩（测试区已置空）
        if not chunk.strip():
            continue
        hits = len(_PANIC_RX.findall(chunk))
        if hits:
            if cat == CAT_JUDGE:
                hf.judge += hits
            else:
                hf.kernel += hits
            for off in sources.count_matches(chunk, _PANIC_RX.pattern)[:2]:
                abs_off = start + off
                if len(hf.samples) < 3:
                    hf.samples.append(f"{sf.line_of(abs_off)}: {sources.snippet_at(sf.text, abs_off, 68)}")

    hf.unsafe_blocks = len(sources.count_matches(sf.code, _UNSAFE_RX.pattern))
    hf.let_discard = len(sources.count_matches(sf.code, _LET_DISCARD_RX.pattern))
    hf.swallow = len(sources.count_matches(sf.code, _SWALLOW_RX.pattern))
    hf.allows = len(sources.count_matches(sf.code, _ALLOW_RX.pattern))

    if sf.test_span:
        a, b = sf.test_span
        hf.test = len(re.findall(_PANIC_RX.pattern, sf.masked[a:b]))
    return hf


@dataclass
class HazardReport:
    files: List[HazardFile] = field(default_factory=list)

    @property
    def kernel(self) -> int:
        return sum(f.kernel for f in self.files)

    @property
    def judge(self) -> int:
        return sum(f.judge for f in self.files)

    @property
    def test(self) -> int:
        return sum(f.test for f in self.files)

    def by_domain(self) -> Dict[str, Dict[str, int]]:
        agg: Dict[str, Dict[str, int]] = defaultdict(lambda: {"kernel": 0, "judge": 0, "test": 0, "files": 0})
        for f in self.files:
            e = agg[f.domain]
            e["kernel"] += f.kernel
            e["judge"] += f.judge
            e["test"] += f.test
            e["files"] += 1
        return dict(agg)

    def worst(self, n: int = 15) -> List[HazardFile]:
        return sorted(self.files, key=lambda f: (-f.kernel, -f.judge))[:n]

    def clean_kernel_files(self) -> int:
        return sum(1 for f in self.files if f.kernel == 0)


def audit(root: str) -> HazardReport:
    rep = HazardReport()
    for sf in sources.iter_sources(root):
        rep.files.append(_scan(sf))
    return rep
