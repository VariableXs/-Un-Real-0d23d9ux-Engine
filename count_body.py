# -*- coding: utf-8 -*-
import re, sys, glob

files = [
    r"docs/unxreal/deepen/C2-B01.md",
    r"docs/unxreal/deepen/C2-B02.md",
    r"docs/unxreal/deepen/C2-B03.md",
    r"docs/unxreal/deepen/C2-B04.md",
    r"docs/unxreal/deepen/C2-B05.md",
]

pat_entry = re.compile(r"^### (UNX-F\d+)")
pat_body = re.compile(r"^(- (?:\*\*正文\*\*|正文)：)(.*)$")

def count_han(s):
    return sum(1 for ch in s if '\u4e00' <= ch <= '\u9fff')

total_deficit = 0
for f in files:
    with open(f, encoding="utf-8") as fh:
        lines = fh.read().splitlines()
    cur = None
    body_text = None
    results = []
    for ln in lines:
        m = pat_entry.match(ln)
        if m:
            if cur is not None:
                results.append((cur, count_han(body_text or "")))
            cur = m.group(1)
            body_text = None
            continue
        m = pat_body.match(ln)
        if m and cur is not None:
            body_text = m.group(2)
    if cur is not None:
        results.append((cur, count_han(body_text or "")))
    bad = [(e, c) for e, c in results if c < 320]
    mn = min((c for _, c in results), default=0)
    print(f"== {f}: 共 {len(results)} 条, 最小汉字数 {mn}, 不达标 {len(bad)} 条")
    for e, c in bad:
        print(f"   {e}: {c} (缺 {320-c})")
    total_deficit += len(bad)
print(f"\n总不达标条数: {total_deficit}")
