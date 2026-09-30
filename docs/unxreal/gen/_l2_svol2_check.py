# -*- coding: utf-8 -*-
"""AI-57 · L2 续卷（B16–B40）七查校验器
查项：1 条数 500；2 ID 连续唯一 F45101–F45600；3 每批 20 条；
4 每批行数 6,000 守恒；5 全卷 150,000；6 判据号唯一（J1..Jn）；7 批头齐整
另附 --full 模式：双册（B01–B15 + B16–B40）合卷 800 条 / 240,000 行总守恒。
"""
import re, sys, hashlib

BOOK = r"docs/Varix/CoRun Varix STAR II · Unxreal/AI-57 · L2 · 500项新功能增补册（B16–B40）.md"
BOOK1 = r"docs/Varix/CoRun Varix STAR II · Unxreal/AI-57 · L2 · 300项新功能增补册（B01–B15）.md"

def load(path, lo, hi):
    text = open(path, encoding="utf-8").read()
    items = re.findall(r"^\| (UNX-F(\d{5})) \| (.+?) \| (\d+) \| 增补 \|", text, re.M)
    jids = re.findall(r"UNX-F(\d{5})-J\d", text)
    return text, items, jids

def check(path, lo, hi, nbatches, label):
    text, items, jids = load(path, lo, hi)
    errs = []
    ids = [int(i[1]) for i in items]
    body_ids = [x for x in ids if lo <= x <= hi]
    # 1 条数
    if len(items) != (hi - lo + 1):
        errs.append(f"条数 {len(items)} != {hi-lo+1}")
    # 2 连续唯一
    if body_ids != list(range(lo, hi + 1)):
        errs.append("ID 不连续或不唯一")
    # 3/4 每批 20 条 + 6000 守恒
    batches = re.split(r"^## 批 UNX-L2-B(\d{2})", text, flags=re.M)
    nb = 0
    for k in range(1, len(batches), 2):
        bn = int(batches[k]); nb += 1
        body = batches[k + 1]
        brows = re.findall(r"^\| UNX-F(\d{5}) \| (.+?) \| (\d+) \| 增补 \|", body, re.M)
        if len(brows) != 20:
            errs.append(f"B{bn:02d} 条数 {len(brows)} != 20")
        tot = sum(int(r[2]) for r in brows)
        if tot != 6000:
            errs.append(f"B{bn:02d} 行数 {tot} != 6000")
        if not (lo <= int(brows[0][0]) <= hi):
            errs.append(f"B{bn:02d} 首条 ID 越界")
    if nb != nbatches:
        errs.append(f"批数 {nb} != {nbatches}")
    # 5 全卷行数
    total = sum(int(i[3]) for i in items)
    expect = (hi - lo + 1) // 20 * 6000
    if total != expect:
        errs.append(f"全卷 {total} != {expect}")
    # 6 判据号唯一（每条目的判据引用全文唯一性：同判据号不跨条复用为本条主判据）
    main_j = re.findall(r"^\| (UNX-F\d{5}) \| .+? \| \d+ \| 增补 \| UNX-F(\d{5})-J1 ", text, re.M)
    seen = {}
    for iid, jid in main_j:
        if jid != iid[-5:]:
            errs.append(f"{iid} 主判据号错位 {jid}")
        seen[jid] = seen.get(jid, 0) + 1
    dups = [j for j, c in seen.items() if c > 1]
    if dups:
        errs.append(f"主判据号重复: {dups[:5]}")
    print(f"[{label}] items={len(items)} total={total} batches={nb} errs={len(errs)}")
    for e in errs:
        print("  ERR:", e)
    return len(errs) == 0, total

ok1, t1 = check(BOOK, 45101, 45600, 25, "续卷 B16–B40")
total_all = t1
if "--full" in sys.argv:
    ok0, t0 = check(BOOK1, 44801, 45100, 15, "首卷 B01–B15")
    total_all = t0 + t1
    print(f"[合卷] items=800 total={total_all} 守恒{'PASS' if total_all == 240000 else 'FAIL'}")
    ok1 = ok1 and ok0 and total_all == 240000
h = hashlib.sha256(open(BOOK, 'rb').read()).hexdigest()[:16]
print("SHA-256[:16]", h)
sys.exit(0 if ok1 else 1)
