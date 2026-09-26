# -*- coding: utf-8 -*-
"""H3 纯功能行计数器 v2：字符串感知（D-30 修正版）。

四分法口径：总行 − 空行 − 注释行 − 测试区行。
D-30 修正：花括号深度追踪前先剥离字符串字面量内容与 // 注释——
format! 模板里的 {} 不再扰乱测试区边界判定。
"""
import os
import sys
import io


def strip_strings_comments(line: str) -> str:
    """剥离字符串字面量内容与 // 注释，保留括号结构。"""
    out = []
    i = 0
    n = len(line)
    in_str = False
    while i < n:
        c = line[i]
        if in_str:
            if c == "\\" and i + 1 < n:
                i += 2
                continue
            if c == '"':
                in_str = False
            i += 1
            continue
        if c == '"':
            in_str = True
            out.append('"')
            i += 1
            continue
        if c == "/" and i + 1 < n and line[i + 1] == "/":
            break
        out.append(c)
        i += 1
    return "".join(out)


def analyze(path: str):
    lines = io.open(path, encoding="utf-8").read().splitlines()
    total = len(lines)
    blank = comment = test = 0
    in_test = False
    depth = 0
    for ln in lines:
        s = ln.strip()
        code = strip_strings_comments(ln).strip()
        if not in_test:
            if code.startswith("#[cfg(test)]"):
                in_test = True
                depth = code.count("{") - code.count("}")
                test += 1
                continue
            if not s:
                blank += 1
            elif s.startswith("//"):
                comment += 1
        else:
            test += 1
            depth += code.count("{") - code.count("}")
            if depth <= 0:
                in_test = False
    return total, blank, comment, test


def main():
    d = sys.argv[1] if len(sys.argv) > 1 else "kernel/varix/src/h3star"
    g = [0, 0, 0, 0]
    rows = {}
    for f in sorted(os.listdir(d)):
        if not f.endswith(".rs"):
            continue
        t, b, c, te = analyze(os.path.join(d, f))
        rows[f] = (t, t - b - c - te)
        g[0] += t
        g[1] += b
        g[2] += c
        g[3] += te
    pure = g[0] - g[1] - g[2] - g[3]
    print("TOTAL", g[0], "PURE", pure,
          "| 50项", pure - rows["mod.rs"][1] - rows["hbase.rs"][1])
    for f, (t, p) in sorted(rows.items()):
        print(f"{f:20s} {t:5d} {p:5d}")


if __name__ == "__main__":
    main()
