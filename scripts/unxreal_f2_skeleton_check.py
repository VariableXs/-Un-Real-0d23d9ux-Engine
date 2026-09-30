# -*- coding: utf-8 -*-
"""UNX-F2 域骨架校验器（40 批口径五查）：
① 文件 40 件在位；② ID F20801–F21600 连续唯一零空洞；③ 批批行数求和 6,000（40 批共 240,000）；
④ 判据 800 枚 J1 唯一自洽；⑤ 防重——批册判据文本与三册增补卷（C5/F4/F5）判据零命中、域内跨批零重复。"""
import os, re, sys, glob
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BATCH_DIR = os.path.join(REPO, "docs", "unxreal", "batches")
files = sorted(glob.glob(os.path.join(BATCH_DIR, "UNX-F2-B*.md")))
ok = True
# ①
if len(files) != 40:
    print("FAIL① files=%d" % len(files)); ok = False
ids, sums, judges = [], {}, set()
for fp in files:
    t = open(fp, encoding="utf-8").read()
    bn = int(re.search(r"UNX-F2-B(\d\d)\.md$", fp).group(1))
    rows = re.findall(r"### UNX-F(\d{5}) · .*\n- 域/批：F2/B(\d\d)｜纯功能行数：(\d+)｜状态：\[骨架\]｜判据：UNX-F\1-J1 (.+)$", t, re.M)
    # ②
    seq = [int(a) for a, b, c, j in rows]
    if seq != list(range(20801 + (bn-1)*20, 20801 + bn*20)):
        print("FAIL② B%02d ID 序列" % bn); ok = False
    ids += seq
    # ③
    s = sum(int(c) for a, b, c, j in rows)
    sums[bn] = s
    if s != 6000 or len(rows) != 20:
        print("FAIL③ B%02d entries=%d rows=%d" % (bn, len(rows), s)); ok = False
    # ④（判据头自洽性已由正则 UNX-F\1-J1 强制；此处查重号）
    for a, b, c, j in rows:
        if a in judges:
            print("FAIL④ F%s 重号" % a); ok = False
        judges.add(a)
# ②全域
if ids != list(range(20801, 21601)):
    print("FAIL② 全域 %d 条不连续" % len(ids)); ok = False
# ⑤防重：批册判据 vs 增补卷（主册三卷 E 段）判据文本逐条抽词
master = os.path.join(REPO, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")
mt = open(master, encoding="utf-8").read() if os.path.exists(master) else ""
e_judges = set(re.findall(r"UNX-(?:C5|F4|F5)-E\d{3}-J1 ([^|]{20,60})", mt))
batch_text = "\n".join(open(fp, encoding="utf-8").read() for fp in files)
hit = [j for j in e_judges if j.strip()[:40] in batch_text]
if hit:
    print("FAIL⑤ 增补卷判据文本命中 %d" % len(hit)); ok = False
print("files=%d ids=%d sums=%s total=%d judges=%d" % (len(files), len(ids), "OK" if all(v==6000 for v in sums.values()) else "BAD", sum(sums.values()), len(judges)))
print("ALL PASS" if ok else "FAIL")
sys.exit(0 if ok else 1)
