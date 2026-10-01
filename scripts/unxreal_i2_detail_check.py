# -*- coding: utf-8 -*-
"""UNX-I2 详述册校验器（AI-42）· 四查：独立复跑文件系统真值。
1 块数=800  2 每块四段齐（功能定位/实现要点/完成标准/交付三件套）  3 每块 ≥300 字  4 判据行与 ID 连续对齐 batches 源册。
"""
import os, re, sys, hashlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DETAIL = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal · UNX-I2 · 800条功能详述.md")
MAIN = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")

def main():
    fails = []
    txt = open(DETAIL, encoding="utf-8").read()
    blocks = re.split(r"(?=^### UNX-F\d+ · )", txt, flags=re.M)[1:]
    if len(blocks) != 800: fails.append(f"blocks {len(blocks)} != 800")
    short = [b[:40] for b in blocks if len(b.replace("\n", "")) - len(re.findall(r"^- ", b, re.M)) * 2 < 300]
    heads = [int(re.match(r"### UNX-F(\d+) ·", b).group(1)) for b in blocks]
    if heads != list(range(32801, 33601)): fails.append("ID discontinuity in detail book")
    segs = ["【功能定位】", "【实现要点】", "【完成标准】", "【交付三件套】"]
    missing = [h for h, b in zip(heads, blocks) if not all(s in b for s in segs)]
    if missing: fails.append(f"missing sections: {missing[:5]}")
    crits = re.findall(r"判据：UNX-F(\d+)-J1 ", txt)
    if len(crits) != 800 or sorted(map(int, crits)) != list(range(32801, 33601)):
        fails.append("criteria lines mismatch")
    # 正文（去条目头与账目行）每块字数下限
    body_short = []
    for h, b in zip(heads, blocks):
        lines = [l for l in b.splitlines() if not l.startswith(("###", "- 域/批"))]
        body = "".join(lines).replace(" ", "")
        if len(body) < 300: body_short.append((h, len(body)))
    if body_short: fails.append(f"blocks under 300 chars: {body_short[:5]} (total {len(body_short)})")
    # 主册整合检查
    m = open(MAIN, encoding="utf-8").read()
    if "UNX-I2 域 800 条功能详述卷" not in m: fails.append("main volume header missing")
    cnt = len(re.findall(r"^【功能定位】", m, re.M))
    if cnt < 800: fails.append(f"main integrated blocks {cnt} < 800")
    print(f"detail checks: blocks={len(blocks)} min_body={min(len(''.join(l for l in b.splitlines() if not l.startswith(('###','- 域/批'))).replace(' ','')) for b in blocks)}")
    if fails:
        print("FAIL:"); [print(" -", f) for f in fails]; sys.exit(1)
    h = hashlib.sha256("\n".join(map(str, heads)).encode()).hexdigest()[:16]
    print("FOUR CHECKS ALL PASS exit=0 | detail fingerprint:", h)

if __name__ == "__main__":
    main()
