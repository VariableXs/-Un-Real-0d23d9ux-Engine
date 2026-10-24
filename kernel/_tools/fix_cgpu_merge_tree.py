#!/usr/bin/env python3
"""Rebuild the balanced CheckSet::merge aggregation tree in cgpu/mod.rs.

WHY this exists
---------------
`cgpu/mod.rs::run_cgpu_checks()` was emitted by an AI assembly pipeline that
appended `crate::checks::CheckSet::merge(` / `),` fragments out of order.
The result has 28 `merge(` openers but 29 leaf `run_*_checks()` calls, i.e. the
parenthesis nesting is NOT balanced -> hard compile error (E0658-ish parse
failure) at the first stray `,`.

HOW it fixes it
---------------
Rather than hand-counting parentheses (fragile, and re-breaks the moment a new
domain is registered), we:

  1. Extract every leaf call `X_checks::run_x_checks(),` in source order.
  2. Emit a canonical left-fold: fold the leaves pairwise into a balanced tree
     of known arity, so the number of `merge(` openers is exactly n-1 and every
     one is closed.
  3. Splice the rebuilt function back in place of the old body, preserving the
     `//!` header banner and the file's module declarations verbatim.

Idempotent: running it twice on an already-balanced file is a no-op.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

TARGET = Path(__file__).resolve().parent.parent / "varix/src/cgpu/mod.rs"
FUNC = "pub fn run_cgpu_checks() -> crate::checks::CheckSet {"
MERGE = "crate::checks::CheckSet::merge"
LEAF_RE = re.compile(r"^\s*([a-z0-9_]+_checks)::(run_[a-z0-9_]+)\(\)[,]?\s*$")


def main() -> int:
    text = TARGET.read_text(encoding="utf-8")

    lines = text.split("\n")
    try:
        start = next(i for i, ln in enumerate(lines) if ln.strip() == FUNC.strip())
    except StopIteration:
        print("FUNC marker not found — aborting (nothing changed)")
        return 1

    # The function body runs until the line that closes it at column 0.
    end = next(i for i in range(start + 1, len(lines)) if lines[i] == "}")

    body = lines[start + 1 : end]
    leaves = [m.group(1) for ln in body if (m := LEAF_RE.match(ln))]
    if not leaves:
        print("no leaf run_* calls found — aborting")
        return 1

    print(f"leaves recovered: {len(leaves)}")

    # Build a balanced tree by repeated pairwise folding. Each fold step emits
    # exactly one `merge(` and one matching `),`, so the emitted text is
    # balanced by construction.
    def emit(nodes: list[str], depth: int = 0) -> list[str]:
        ind = "        " + "    " * depth
        out: list[str] = []
        i = 0
        while i < len(nodes):
            if i + 1 < len(nodes):
                out.append(f"{ind}{MERGE}(")
                out.append(f"{ind}    {nodes[i]},")
                out.append(f"{ind}    {nodes[i + 1]},")
                out.append(f"{ind}),")
                i += 2
            else:
                out.append(f"{ind}{nodes[i]},")
                i += 1
        return out

    # Preserve each leaf's exact source spelling: module name and check fn name
    # do not always follow the same suffix rule (e.g.
    # `vcq01_reliability_checks::run_vcq01_checks`).
    level: list[str] = []
    for ln in body:
        m = LEAF_RE.match(ln)
        if m:
            level.append(f"{m.group(1)}::{m.group(2)}()")

    # Build a provably balanced tree by recursive halving of the leaf list.
    # A range of n leaves always emits exactly n-1 `merge(` openers. Every
    # INTERNAL node is an argument of its parent, so it needs a trailing comma;
    # the ROOT node is the tail expression of the function body, so it must NOT
    # be followed by a comma (that is exactly the `expected one of ., ; ?, }`
    # parse error this script exists to eliminate).
    def build(lo: int, hi: int, depth: int, is_root: bool) -> str:
        ind = "        " + "    " * depth
        if hi - lo == 1:
            leaf = f"{ind}{level[lo]}"
            return leaf if is_root else leaf + ","
        mid = (lo + hi) // 2
        left = build(lo, mid, depth + 1, False)
        right = build(mid, hi, depth + 1, False)
        tail = "" if is_root else ","
        return f"{ind}{MERGE}(\n{left}\n{right}\n{ind}){tail}"

    new_body = [build(0, len(level), 0, True)]
    new_lines = lines[: start + 1] + new_body + ["}"] + lines[end + 1 :]
    TARGET.write_text("\n".join(new_lines), encoding="utf-8")
    print(f"rebuilt run_cgpu_checks with {len(level)} leaves, balanced")
    return 0


if __name__ == "__main__":
    sys.exit(main())