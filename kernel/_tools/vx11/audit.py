#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""VX11 静态合同审计：行数 / 判据数 / 测试数 / 红线 / 头 / SEG / 函数名。"""
import io
import json
import os
import re
import sys

KERNEL = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\kernel"
MODS = os.path.join(KERNEL, r"varix\src\unxstar")
PLAN = os.path.join(KERNEL, r"_tools\vx11\plan.json")

BANNED = [
    (re.compile(r"\bstd::"), "std::"),
    (re.compile(r"\bunsafe\b"), "unsafe"),
    (re.compile(r"\.unwrap\(\)"), "unwrap"),
    (re.compile(r"\.expect\("), "expect"),
    (re.compile(r"\bpanic!\s*\("), "panic!"),
    (re.compile(r"\bassert!\s*\("), "assert!"),
    (re.compile(r"\bf32\b|\bf64\b"), "float"),
]


def audit(card):
    path = os.path.join(MODS, card["stem"] + ".rs")
    row = {"id": card["id"], "stem": card["stem"], "ok": False, "why": []}
    if not os.path.exists(path):
        row["why"].append("MISSING")
        return row
    src = io.open(path, encoding="utf-8", errors="replace").read()
    lines = src.count("\n") + (0 if src.endswith("\n") else 1)
    row["lines"] = lines
    row["tests"] = len(re.findall(r"#\[test\]", src))
    row["adds"] = len(re.findall(r"\bs\.add(?:_dyn)?\(", src))
    row["checks"] = len(re.findall(r"\bs\.add\(", src))
    row["dyn"] = len(re.findall(r"\bs\.add_dyn\(", src))
    head = src.splitlines()[0] if src else ""
    if not head.startswith("//! UNX-F%d " % card["id"]):
        row["why"].append("HEAD")
    seg = "0x%X" % card["seg"]
    if not re.search(r"pub const SEG_BASE: u32 = %s;" % seg, src, re.I):
        row["why"].append("SEG")
    fname = "pub fn run_unx%d_checks" % card["id"]
    if src.count(fname) != 1:
        row["why"].append("FN")
    sig_ok = ("pub fn run_unx%d_checks() -> checks::CheckSet" % card["id"] in src
              or "pub fn run_unx%d_checks() -> CheckSet" % card["id"] in src)
    if not sig_ok:
        row["why"].append("FNSIG")
    cut = src.find("#[cfg(test)]")
    prod = src if cut < 0 else src[:cut]
    for rx, tag in BANNED:
        if rx.search(prod):
            row["why"].append("BAN:" + tag)
    if row["tests"] < 8:
        row["why"].append("T<8")
    if row["adds"] < 12:
        row["why"].append("C<12")
    if row["lines"] < 700:
        row["why"].append("L<700")
    row["ok"] = not row["why"]
    return row


def main():
    plan = json.load(io.open(PLAN, encoding="utf-8"))
    lo = int(sys.argv[1]) if len(sys.argv) > 1 else 9202
    hi = int(sys.argv[2]) if len(sys.argv) > 2 else 9401
    rows = [audit(c) for c in plan if lo <= c["id"] <= hi]
    bad = [r for r in rows if not r["ok"]]
    have = [r for r in rows if "lines" in r]
    print("cards=%d files=%d ok=%d bad=%d" % (len(rows), len(have), len(rows) - len(bad), len(bad)))
    if have:
        print("lines min=%d max=%d sum=%d tests=%d checks=%d" % (
            min(r["lines"] for r in have), max(r["lines"] for r in have),
            sum(r["lines"] for r in have),
            sum(r["tests"] for r in have), sum(r["adds"] for r in have)))
    for r in bad:
        print("BAD %d %s %s" % (r["id"], r["stem"], ",".join(r["why"])))


if __name__ == "__main__":
    main()
