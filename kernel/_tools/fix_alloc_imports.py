#!/usr/bin/env python3
"""One-shot repair: inject missing `alloc` imports into no_std modules.

Strategy (surgical, per-file, evidence-driven):
  1. Parse `cargo build --message-format=short` error lines.
  2. Per file, determine the exact set of missing items:
       - type not found: Vec / String / Box
       - method not found: to_string / to_owned   -> alloc::string::ToString
       - macro not found: format! / vec! / println-like
  3. Skip injection if the file ALREADY imports that item by any path
     (so we never create E0252 duplicate-definition errors).
  4. Insert one `use alloc::...;` block immediately after the last
     leading `//!` doc-comment line of the file header, preserving the
     existing banner comments.
"""
from __future__ import annotations

import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
KERNEL = ROOT  # kernel/


def collect_errors() -> list[str]:
    proc = subprocess.run(
        ["cargo", "build", "--release", "--message-format=short"],
        cwd=KERNEL, capture_output=True, text=True, encoding="utf-8", errors="replace",
    )
    out = proc.stdout + proc.stderr
    errs = []
    for line in out.splitlines():
        m = re.match(r"^(varix\\?src[\\/].*?):\d+:\d+:\s+error", line)
        if m:
            errs.append(line)
    return errs


TYPE_ITEMS = {
    "Vec": "alloc::vec::Vec",
    "String": "alloc::string::String",
    "Box": "alloc::boxed::Box",
}
# method -> trait providing it
METHOD_TRAIT = {
    "to_string": "alloc::string::ToString",
    "to_owned": "alloc::borrow::ToOwned",
}
# macro -> import path
MACRO_ITEMS = {
    "format": "alloc::format",
    "vec": "alloc::vec",
}


def parse(errs: list[str]) -> dict[str, set[str]]:
    need: dict[str, set[str]] = defaultdict(set)
    for line in errs:
        fm = re.match(r"^(.*?):\d+:\d+:\s+error(\[(E\d+)\])?", line)
        if not fm:
            continue
        rel = fm.group(1).replace("\\", "/")
        code = fm.group(3) or ""
        if code in ("E0425", "E0433", "E0412", "E0405"):
            m = re.search(r"cannot find (?:type|value|macro) `([A-Za-z_][A-Za-z0-9_]*)`", line) \
                or (re.search(r"not found in this scope", line) and re.search(r"`([A-Za-z_][A-Za-z0-9_]*)`", line))
            if m and m.group(1) in TYPE_ITEMS:
                need[rel].add(TYPE_ITEMS[m.group(1)])
            elif m:
                need[rel].add(f"??{m.group(1)}")
        elif code == "E0599":
            m = re.search(r"no method named `([A-Za-z_0-9]+)`", line)
            if m and m.group(1) in METHOD_TRAIT:
                need[rel].add(METHOD_TRAIT[m.group(1)])
        elif code == "E0609" or code == "E0061":
            need[rel].add("??macro")
        else:
            # un-annotated errors: `cannot find macro \`vec\` in this scope`
            m = re.search(r"cannot find macro `([A-Za-z_][A-Za-z0-9_]*)`", line)
            if m and m.group(1) in MACRO_ITEMS:
                need[rel].add(MACRO_ITEMS[m.group(1)])
    return need


HEADER_RE = re.compile(r"^\s*use\s+([A-Za-z_][A-Za-z0-9_]*)")


def already_imported(text: str, item: str) -> bool:
    """True if `item` is already bound by an existing top-level `use`.

    Compares the FULL path tail, so `alloc::vec` (the macro) and
    `alloc::vec::Vec` (the type) are correctly treated as distinct items
    even though their last segments would collide under a naive `seg` test.
    """
    tail = item.removeprefix("alloc::")          # e.g. "vec" or "vec::Vec"
    seg = tail.rsplit("::", 1)[-1]                # e.g. "vec" or "Vec"
    for m in re.finditer(r"^\s*(?:pub\s+)?use\s+([^;]+);", text, re.M):
        clause = m.group(1)
        # `use alloc::{vec, vec::Vec};`  -> expand brace groups
        for br in re.finditer(r"\{([^}]*)\}", clause):
            prefix = clause.split("{")[0].rstrip(":")
            for part in br.group(1).split(","):
                part = part.strip().split(" as ")[0].strip()
                if not part:
                    continue
                full = f"{prefix}::{part}" if prefix else part
                if full.removeprefix("alloc::") == tail or part == seg and len(tail) == len(seg):
                    return True
        # single-path `use alloc::vec;` / `use alloc::vec::Vec;`
        top = clause.strip().split(" as ")[0].strip()
        if top == f"alloc::{tail}" or top == tail:
            return True
        if clause.strip() == "*":
            return True
    return False


def insert_after_header(text: str, uses: list[str]) -> str:
    lines = text.split("\n")
    # find end of leading //! banner
    idx = 0
    for i, ln in enumerate(lines):
        s = ln.strip()
        if s.startswith("//!"):
            idx = i + 1
        elif s == "" and i == 0:
            continue
        else:
            break
    # collect contiguous blank lines after banner to keep formatting tidy
    block = "\n".join(f"use {u};" for u in sorted(uses))
    lines.insert(idx, block)
    return "\n".join(lines)


def main() -> int:
    errs = collect_errors()
    if not errs:
        print("NO ERRORS — nothing to do")
        return 0
    need = parse(errs)
    unknown = sorted({i for v in need.values() for i in v if i.startswith("??")})
    if unknown:
        print("UNRESOLVED tokens (need manual work):")
        for u in unknown:
            print("  ", u)
    changed = 0
    for rel, items in sorted(need.items()):
        items = {i for i in items if not i.startswith("??")}
        if not items:
            continue
        p = KERNEL / rel
        if not p.exists():
            print("MISSING FILE:", rel)
            continue
        text = p.read_text(encoding="utf-8", errors="replace")
        todo = [i for i in sorted(items) if not already_imported(text, i)]
        if not todo:
            continue
        p.write_text(insert_after_header(text, todo), encoding="utf-8")
        changed += 1
        print(f"[fix] {rel}: +{', '.join(t.removeprefix('alloc::') for t in todo)}")
    print(f"\nchanged {changed} files")
    return 0


if __name__ == "__main__":
    sys.exit(main())