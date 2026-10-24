# -*- coding: utf-8 -*-
"""源码索引层：Rust 词法遮罩、测试区切分、顶层条目定位。

为什么不用正则一把梭：审计要区分「代码里真的调了 `unwrap()`」与「文档/字符串里
提到 unwrap」。正则做不到这一点，会把 156 个模块的文档头全部误判成违规——上一轮
人工核验时正是踩在这里（`unx2562` 头部写着「零 unwrap()」，正则直接命中它自己）。

因此本模块做一件很窄的事：**产出一份与原文等长的遮罩文本**，其中注释内容与字符串
内容全部替换成空格（换行保留，行号不漂移）。后续所有判据都在遮罩文本上跑正则，
命中的偏移量可以直接回原文取上下文。

支持：
  * 行注释 `//`、块注释 `/* */`（Rust 允许嵌套）
  * 普通字符串 `"..."`（含 `\\` 转义）、字节串 `b"..."`
  * 原始字符串 `r"..."` / `r#"..."#` / `r##"..."##`（含 `br#""#` 变体）
  * 字符字面量 `'a'` / `'\\n'` / `'\\u{1F600}'`（与生命周期 `'a` 区分）
"""

from __future__ import annotations

import os
import re
from dataclasses import dataclass, field
from typing import Dict, Iterable, Iterator, List, Optional, Sequence, Tuple


# ---------------------------------------------------------------------------
# 词法遮罩
# ---------------------------------------------------------------------------

def mask_rust(text: str) -> str:
    """把注释与字符串内容替换成空格，保持长度与换行不变。

    返回串与入参等长：`masked[i] == ' '` 表示原文该位置属于注释/字符串内容，
    否则与原文同字符。调用方可直接用同一偏移量在两者间互查。
    """
    n = len(text)
    out = list(text)
    i = 0

    def blank(a: int, b: int) -> None:
        for k in range(a, min(b, n)):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = text[i]
        nxt = text[i + 1] if i + 1 < n else ""

        # ---- 行注释 -------------------------------------------------------
        if c == "/" and nxt == "/":
            j = text.find("\n", i)
            j = n if j < 0 else j
            blank(i, j)
            i = j
            continue

        # ---- 块注释（支持嵌套）--------------------------------------------
        if c == "/" and nxt == "*":
            depth = 1
            j = i + 2
            while j < n and depth > 0:
                if text.startswith("/*", j):
                    depth += 1
                    j += 2
                elif text.startswith("*/", j):
                    depth -= 1
                    j += 2
                else:
                    j += 1
            blank(i, j)
            i = j
            continue

        # ---- 字符字面量 ---------------------------------------------------
        # 只有 `'x'` / `'\n'` / `'\u{...}'` 才是字面量；`'a` 紧跟标识符是生命周期。
        if c == "'":
            j = i + 1
            if j < n and text[j] == "\\":
                k = j + 1
                if k < n and text[k] == "u" and text.startswith("u{", k):
                    close = text.find("}", k)
                    k = n if close < 0 else close + 1
                else:
                    k += 1
                if k < n and text[k] == "'":
                    blank(i + 1, k)
                    i = k + 1
                    continue
            elif j + 1 < n and text[j + 1] == "'":
                blank(i + 1, j)
                i = j + 2
                continue
            i += 1
            continue

        # ---- 原始字符串 r"..." / r#"..."# / br#"..."# ----------------------
        if c in "rb":
            m = _RAW_STR_RE.match(text, i)
            if m:
                body_start = m.end()
                hashes = m.group("hash") or ""
                terminator = '"' + hashes
                end = text.find(terminator, body_start)
                end = n if end < 0 else end + len(terminator)
                blank(m.start() + (1 if c == "b" else 0), end - len(hashes) - 1)
                i = end
                continue

        # ---- 普通字符串 / 字节串 ------------------------------------------
        if c == '"':
            j = i + 1
            while j < n:
                if text[j] == "\\":
                    j += 2
                    continue
                if text[j] == '"':
                    break
                j += 1
            blank(i + 1, j)
            i = j + 1
            continue

        i += 1

    return "".join(out)


# `r"` / `r#*"` / `br"` / `br#*"` 的头部匹配；group("hash") 捕获 `#` 串。
_RAW_STR_RE = re.compile(r'(?:b?r)(?P<hash>#{0,255})"')


def strip_comments(text: str) -> str:
    """只剥注释、保留字符串（用于需要看字符串内容的场景，如诊断码文本）。"""
    masked = mask_rust(text)
    out = []
    for a, b in zip(text, masked):
        out.append(a if b != " " or a in " \t\n" else " ")
    return "".join(out)


# ---------------------------------------------------------------------------
# 括号匹配
# ---------------------------------------------------------------------------

_PAIRS = {"{": "}", "(": ")", "[": "]"}


def matching_brace(masked: str, open_idx: int) -> int:
    """给定遮罩文本中某个开括号下标，返回配对闭括号下标；找不到返回 len-1。"""
    opener = masked[open_idx]
    closer = _PAIRS[opener]
    depth = 0
    for i in range(open_idx, len(masked)):
        ch = masked[i]
        if ch == opener:
            depth += 1
        elif ch == closer:
            depth -= 1
            if depth == 0:
                return i
    return len(masked) - 1


def top_level_items(masked: str) -> List[Tuple[int, int, str]]:
    """切出文件顶层的条目区间，返回 [(start, end, kind)]。

    `kind` 取 `fn` / `struct` / `enum` / `impl` / `trait` / `mod` / `const` /
    `static` / `type` / `use` / `macro` / `attr` / `other`。
    判定方式：深度 0 上遇到的第一个关键字；条目区间到同深度下一个条目起点为止。
    """
    starts: List[Tuple[int, int, str]] = []
    depth = 0
    i = 0
    n = len(masked)
    # 只在「上一个字符是换行」时认条目起点，避免把 `let x = struct` 之类算进来。
    while i < n:
        ch = masked[i]
        if ch in "{([":
            depth += 1
            i += 1
            continue
        if ch in "})]":
            depth -= 1
            i += 1
            continue
        if depth == 0 and (i == 0 or masked[i - 1] == "\n"):
            m = _ITEM_HEAD_RE.match(masked, i)
            if m:
                # 跳过 `pub` / `pub(crate)` / `unsafe` / `async` / `const fn` 前缀
                starts.append((i, 0, m.group("kind")))
                # 直接把指针移到该条目首个 `{` / `;` 之后，避免把块内内容当条目
                j = i
                while j < n and masked[j] not in "{;":
                    j += 1
                if j >= n:
                    break
                if masked[j] == "{":
                    end = matching_brace(masked, j)
                    i = end + 1
                else:
                    i = j + 1
                continue
        i += 1

    out: List[Tuple[int, int, str]] = []
    for idx, (start, _, kind) in enumerate(starts):
        end = starts[idx + 1][0] if idx + 1 < len(starts) else n
        out.append((start, end, kind))
    return out


_ITEM_HEAD_RE = re.compile(
    r"(?:(?:pub(?:\s*\([^)]*\))?|unsafe|async|extern(?:\s+\"[^\"]*\")?|const|default)\s+)*"
    r"(?P<kind>fn|struct|enum|union|impl|trait|mod|macro_rules|macro|const|static|type|use|let)\b"
)


# ---------------------------------------------------------------------------
# 测试区
# ---------------------------------------------------------------------------

_TEST_GATE_RE = re.compile(r"#\[cfg\((?:test|all\(test[^)]*\))\)\]\s*(?:#\[[^\]]*\]\s*)*mod\s+\w+\s*\{")


def test_region(masked: str) -> Optional[Tuple[int, int]]:
    """返回 `#[cfg(test)] mod tests { ... }` 的区间；没有则 None。

    内核里所有测试都写成这一种形态（全仓 13k+ 处），只认这一种即可，
    比逐个解析 `#[cfg]` 求值简单得多，也不会误伤 `#[cfg(feature = ...)]`。
    """
    m = _TEST_GATE_RE.search(masked)
    if not m:
        return None
    brace = masked.find("{", m.end() - 1)
    if brace < 0:
        return None
    return (m.start(), matching_brace(masked, brace))


# ---------------------------------------------------------------------------
# 文件模型
# ---------------------------------------------------------------------------

@dataclass
class SourceFile:
    """单个 .rs 文件的三种视图：原文 / 遮罩 / 生产区。"""

    path: str
    rel: str
    text: str
    masked: str
    code: str                    # 剥掉测试区后的遮罩文本（等长，测试区置空）
    test_span: Optional[Tuple[int, int]] = None

    @property
    def domain(self) -> str:
        parts = self.rel.split("/")
        return parts[0] if len(parts) > 1 else "(root)"

    @property
    def is_check_layer(self) -> bool:
        name = os.path.basename(self.rel)
        return (
            name.endswith("_checks.rs")
            or name.endswith("_test.rs")
            or self.rel.startswith("tests/")
            or self.domain in ("checks",)
        )

    def production_text(self) -> str:
        """生产区原文（测试区替换为空行，行号仍对齐）。"""
        if not self.test_span:
            return self.text
        a, b = self.test_span
        return self.text[:a] + "\n" * self.text[a:b].count("\n") + self.text[b:]

    def line_of(self, offset: int) -> int:
        return self.text.count("\n", 0, offset) + 1


def load(root: str, rel: str) -> Optional[SourceFile]:
    path = os.path.join(root, rel.replace("/", os.sep))
    try:
        text = open(path, encoding="utf-8", errors="replace").read()
    except OSError:
        return None
    masked = mask_rust(text)
    span = test_region(masked)
    code = masked
    if span:
        a, b = span
        code = masked[:a] + " " * (b - a) + masked[b:]
    return SourceFile(path=path, rel=rel, text=text, masked=masked, code=code, test_span=span)


def iter_sources(root: str) -> Iterator[SourceFile]:
    """遍历源码树，按路径排序（输出稳定，便于 diff 报告）。"""
    rels: List[str] = []
    for dp, dn, fn in os.walk(root):
        dn.sort()
        for f in sorted(fn):
            if f.endswith(".rs"):
                rel = os.path.relpath(os.path.join(dp, f), root).replace(os.sep, "/")
                rels.append(rel)
    for rel in sorted(rels):
        sf = load(root, rel)
        if sf is not None:
            yield sf


# ---------------------------------------------------------------------------
# 计数小工具
# ---------------------------------------------------------------------------

def count_matches(masked: str, pattern: str) -> List[int]:
    """在遮罩文本上跑正则，返回全部命中起始偏移。"""
    return [m.start() for m in re.finditer(pattern, masked)]


def snippet_at(text: str, offset: int, width: int = 90) -> str:
    """取命中处所在行的片段，用于报告里贴证据。"""
    start = text.rfind("\n", 0, offset) + 1
    end = text.find("\n", offset)
    end = len(text) if end < 0 else end
    line = text[start:end].strip()
    return line[:width]
