# -*- coding: utf-8 -*-
"""文档承诺的抽取与核验。

内核 150+ 个模块的文件头文档里逐条写着实现纪律，例如：

    //! `no_std + alloc`；零 `unwrap()` / `expect()` / `panic!` / `todo!()`；
    //!   零浮点、零真实 IO、零墙钟。
    //! - 随机性一律为确定性 LCG：
    //!   `state = state.wrapping_mul(6364136223846793005).wrapping_add(...)`
    //! - 指纹为冻结 FNV-1a：basis `0xcbf29ce484222325`，prime `0x100000001b3`。

这些是**契约**，不是注释。契约被违反了要能报警，否则它只是一句吉祥话。
本模块把每个承诺编译成一个可执行判据（`Claim.kind` 决定用哪条检查），
逐文件求值，违规处带行号与原文片段。

判据一律只在**生产区**（剥掉 `#[cfg(test)]` 之后）求值——测试代码里 unwrap 是
宿主侧的事，与内核镜像无关。判据层文件单独标注，因为它们的 unwrap 属于
「判据自身写错」而非「内核会崩」，需要不同处置。
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Dict, Iterable, List, Optional, Sequence, Tuple

from . import sources
from .sources import SourceFile


# ---------------------------------------------------------------------------
# 承诺模型
# ---------------------------------------------------------------------------

@dataclass
class Claim:
    """一条可执行承诺。`kind` 决定核验方式。"""

    kind: str            # "no_panic" / "no_float" / "lcg" / "fnv"
    text: str            # 原文承诺片段（写进报告，便于人工对照）
    line: int            # 承诺所在行号


@dataclass
class Violation:
    line: int
    snippet: str


@dataclass
class ClaimResult:
    rel: str
    domain: str
    claim: Claim
    ok: bool
    hits: List[Violation] = field(default_factory=list)
    note: str = ""

    @property
    def count(self) -> int:
        return len(self.hits)


# 冻结常量：模块文档一旦写出这两个数，指纹就不可再改。
_LCG_MUL = "6364136223846793005"
_LCG_ADD = "1442695040888963407"
_FNV_BASIS = "0xcbf29ce484222325"
_FNV_PRIME = "0x100000001b3"

# 承诺识别的锚点。这里的原则是**宁缺勿滥**：一条判据如果会误报，它就比没有更糟
# ——报告会被噪音淹没，真问题反而沉底。所以每条锚点都要求「文件自己在声明」，
# 而不是「碰巧提到了这个词」。实测三类误报样本：
#   `appmgr.rs`「FNV-1a 32 位内容哈希」    → 32 位变体，不是冻结的 64 位
#   `vee04_checks.rs`「整数表零浮点」      → 描述实现手段，不是全域禁浮点
#   `float.rs`「零浮点条款是模块级纪律」   → 正在解释这条纪律的适用范围
_CLAIM_PATTERNS: Sequence[Tuple[str, str]] = (
    # 边界声明句式：「零 `unwrap()` / `expect()` / ...」
    ("no_panic", r"零\s*`?unwrap"),
    # 边界声明句式：「零浮点、零真实 IO、零墙钟」——三个一起写才算本模块自述
    ("no_float", r"零浮点[、,]\s*零(?:真实\s*)?IO"),
    # 只有文档里同时给出常量，才做常量核验（否则指纹可能来自共享模块，静态判不了）
    ("lcg", _LCG_MUL),
    ("fnv", r"FNV-1a[^\n]{0,80}(?:basis|prime)"),
)

# 只在文件头找承诺：前 200 行足够覆盖全部模块（最长文档头 ~120 行）。
HEADER_LINES = 200


def extract_claims(sf: SourceFile) -> List[Claim]:
    """从文件头文档里抽出全部承诺。"""
    head = sf.text.splitlines()[:HEADER_LINES]
    out: List[Claim] = []
    seen = set()
    for kind, pat in _CLAIM_PATTERNS:
        rx = re.compile(pat)
        for i, line in enumerate(head, 1):
            if kind in seen:
                break
            m = rx.search(line)
            if m:
                out.append(Claim(kind=kind, text=m.group(0).strip(), line=i))
                seen.add(kind)
    return out


# ---------------------------------------------------------------------------
# 各类核验器
# ---------------------------------------------------------------------------

# panic 家族：`unwrap()` / `expect(` / `panic!` / `todo!` / `unimplemented!`。
# 用 `\.unwrap\s*\(` 而不是裸 `unwrap`，避开 `unwrap_or` / `unwrap_or_else` /
# `unwrap_or_default`——那三兄弟是**正确**的降级写法，不该计入。
_PANIC_RX = re.compile(r"\.unwrap\s*\(|\.expect\s*\(|\bpanic!\s*\(|\btodo!\s*\(|\bunimplemented!\s*\(")

# 浮点在裸目标上是硬伤：`x86_64-unknown-none` 无 libc，core 不含 transcendentals。
# 这里只查「显式浮点」痕迹：类型标注、字面量后缀、浮点方法调用。
_FLOAT_RX = re.compile(
    r"\bf32\b|\bf64\b|(?<![\w.])\d+\.\d+(?:[eE][-+]?\d+)?f?(?:32|64)?\b|\.m_(?:sqrt|sin|cos|tan|exp|ln|powf|atan2?)\s*\("
)


def _hits(sf: SourceFile, rx: re.Pattern) -> List[Violation]:
    """在**生产区**遮罩文本上找命中，回原文取行号与片段。"""
    out: List[Violation] = []
    for off in sources.count_matches(sf.code, rx.pattern):
        out.append(Violation(line=sf.line_of(off), snippet=sources.snippet_at(sf.text, off)))
    return out


def verify(sf: SourceFile, claim: Claim) -> ClaimResult:
    res = ClaimResult(rel=sf.rel, domain=sf.domain, claim=claim, ok=True)

    if claim.kind == "no_panic":
        res.hits = _hits(sf, _PANIC_RX)
        if res.hits and sf.is_check_layer:
            res.note = "判据层：unwrap 属判据自身健壮性问题，不影响内核镜像"
        elif res.hits:
            res.note = "实现层：裸目标 panic = 内核崩溃，必须改为显式降级"

    elif claim.kind == "no_float":
        res.hits = _hits(sf, _FLOAT_RX)
        if res.hits:
            res.note = "裸目标缺 libm transcendentals；应走 crate::float::FloatExt"

    elif claim.kind == "lcg":
        body = sf.code
        # 委托给共享 `lcg_step()` 与内联常量都是合法实现；只有两者皆无才算失约。
        if "lcg_step" in body:
            res.ok = True
            res.note = "委托共享 lcg_step"
        else:
            missing = [n for n, lit in (("mul", _LCG_MUL), ("add", _LCG_ADD)) if lit not in body]
            if missing:
                res.ok = False
                res.note = "缺 LCG 常量：" + "/".join(missing) + "，且未调用 lcg_step"
                res.hits = [Violation(line=claim.line, snippet=claim.text)]

    elif claim.kind == "fnv":
        body = sf.code.lower()
        if "fnv1a" in body:
            res.ok = True
            res.note = "委托 fnv1a 共享实现"
        else:
            missing = [n for n, lit in (("basis", _FNV_BASIS), ("prime", _FNV_PRIME)) if lit not in body]
            if missing:
                res.ok = False
                res.note = "缺 FNV 常量：" + "/".join(missing) + "，且未调用 fnv1a"
                res.hits = [Violation(line=claim.line, snippet=claim.text)]

    res.ok = not res.hits
    return res


# ---------------------------------------------------------------------------
# 批量入口
# ---------------------------------------------------------------------------

def audit(root: str) -> Tuple[List[ClaimResult], int]:
    """扫全树，返回 (违规结果列表, 被检查文件数)。只返回未通过的条目。"""
    bad: List[ClaimResult] = []
    files = 0
    for sf in sources.iter_sources(root):
        claims = extract_claims(sf)
        if not claims:
            continue
        files += 1
        for c in claims:
            r = verify(sf, c)
            if not r.ok:
                bad.append(r)
    bad.sort(key=lambda r: (-r.count, r.rel))
    return bad, files


def summarize(results: Iterable[ClaimResult]) -> Dict[str, int]:
    """按承诺种类汇总违规数，供报告抬头使用。"""
    agg: Dict[str, int] = {}
    for r in results:
        agg[r.claim.kind] = agg.get(r.claim.kind, 0) + 1
    return agg
