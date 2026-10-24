#!/usr/bin/env python3
"""brace_drift.py — 复现 fuck-u-code regex-parser 的 braceStack 漂移并定位漂移源。

用法: python brace_drift.py <file.rs>
输出: 每行累计 brace 深度中异常跳变的行（字符串/字符字面量内的大括号）。
"""
import re
import sys

STR = re.compile(r'"(?:[^"\\]|\\.)*"')
CHR = re.compile(r"'(?:[^'\\]|\\.)'")
LIFETIME = re.compile(r"'[a-z_][a-z0-9_]*")


def analyze(path: str) -> None:
    lines = open(path, encoding="utf-8").read().split("\n")
    depth = 0
    events = []
    for i, l in enumerate(lines):
        t = l.strip()
        if t.startswith("//"):
            continue  # 解析器跳过注释行（含 //! ///）
        # 剔除 char 字面量与字符串（与解析器的差异：解析器并不剔除——这正是漂移源）
        nos = CHR.sub("C", STR.sub('"S"', l))
        nos = LIFETIME.sub("L", nos)
        o = nos.count("{")
        c = nos.count("}")
        # 解析器口径（不剔除字符串/char）
        raw_o = l.count("{")
        raw_c = l.count("}")
        drift = (raw_o - raw_c) - (o - c)
        if drift != 0 and t:
            events.append((i + 1, drift, t[:76]))
    print("suspect lines (raw vs literal-stripped brace delta mismatch):")
    for ln, d, txt in events[:40]:
        sign = "+" if d > 0 else ""
        print(f"  L{ln} drift={sign}{d}  {txt}")
    print(f"total suspect: {len(events)}")


if __name__ == "__main__":
    analyze(sys.argv[1])
