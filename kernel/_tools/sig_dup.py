#!/usr/bin/env python3
"""sig_dup.py — 复刻 fuck-u-code code-duplication 的控制流签名算法, 列出同签名函数组。

用法: python sig_dup.py <file.rs>
"""
import re
import sys

FN_PAT = re.compile(r"^\s*(?:pub(?:\((?:\w+|super|crate)\))?\s+)?(?:const\s+)?(?:unsafe\s+)?(?:extern\s+\"[^\"]+\"\s+)?fn\s+(\w+)")


def signature(lines, start, end):
    toks = []
    for l in lines[start - 1 : end]:
        t = l.strip()
        if not t or t.startswith("//"):
            continue
        if re.match(r"^if\s*\(", t) or re.match(r"^}\s*else\s+if\s*\(", t):
            toks.append("I")
        elif re.match(r"^for\s*[\s(]", t):
            toks.append("F")
        elif re.match(r"^while\s*\(", t):
            toks.append("W")
        elif re.match(r"^switch\s*\(", t):
            toks.append("S")
        elif re.match(r"^case\s+", t):
            toks.append("C")
        elif re.match(r"^return\b", t):
            toks.append("R")
        elif re.match(r"^(const|let|var)\s+\w", t) or ("=" in t and not re.search(r"[=!<>]=", t)):
            toks.append("A")
    return "".join(toks)


def main():
    lines = open(sys.argv[1], encoding="utf-8").read().split("\n")
    fns = []  # (name, start, end)
    cur = None
    depth = 0
    for i, l in enumerate(lines):
        t = l.strip()
        if cur is None:
            m = FN_PAT.match(t if not t.startswith("//") else "")
            if m:
                cur = [m.group(1), i + 1, 0]
        if t and not t.startswith("//"):
            depth += l.count("{") - l.count("}")
        if cur is not None and depth <= 0 and i + 1 > cur[1]:
            fns.append((cur[0], cur[1], i + 1))
            cur = None
        elif cur is not None and depth == 0 and "}" in l and i + 1 > cur[1]:
            fns.append((cur[0], cur[1], i + 1))
            cur = None
    groups = {}
    for name, s, e in fns:
        sig = signature(lines, s, e)
        if len(sig) >= 3:
            groups.setdefault(sig, []).append(name)
    for sig, names in sorted(groups.items(), key=lambda kv: -len(kv[1])):
        if len(names) > 1:
            print(f"DUP {sig}  -> {names}")
    print(f"total fns: {len(fns)}, dup groups: {sum(1 for v in groups.values() if len(v)>1)}")


if __name__ == "__main__":
    main()
