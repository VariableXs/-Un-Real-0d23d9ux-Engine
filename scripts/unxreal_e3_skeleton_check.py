# -*- coding: utf-8 -*-
"""UNX-E3 骨架立账机械校验器（AI-23 · B01–B15 · 300 条）
断言链：①批册 15 件、条数 20/批 ②ID F17601–F17900 连续唯一 ③批批求和恰 6,000、域 90,000
④行数 200–400 ⑤判据号唯一且与条目 ID 一一对应 ⑥防重 grep 五范围零撞号 ⑦批头区间与实际一致
exit 0 = 全绿
"""
import os, re, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
BATCH = os.path.join(REPO, "docs", "unxreal", "batches")

fails = []
H1 = re.compile(r"^# UNX-E3-B(\d{2}) · .*（F(\d+)[–\-—]F(\d+)")
HEAD = re.compile(r"^### UNX-F(\d+) · (.+)$")
META = re.compile(r"^- 域/批：E3/B\d{2}｜纯功能行数：(\d+)｜状态：\[骨架\]｜判据：(UNX-F\d+-J1) ")

books = sorted(f for f in os.listdir(BATCH) if f.startswith("UNX-E3-B") and f.endswith(".md"))
if len(books) != 15:
    fails.append("book-count=%d != 15" % len(books))

all_ids, all_jids, total = [], [], 0
for fn in books:
    lines = open(os.path.join(BATCH, fn), encoding="utf-8").read().splitlines()
    h1 = next((l for l in lines if l.startswith("# ")), "")
    m = H1.match(h1)
    if not m:
        fails.append("%s: H1 unparsed" % fn)
        continue
    bno, f1, f2 = int(m.group(1)), int(m.group(2)), int(m.group(3))
    ids, rows, jids = [], [], []
    for l in lines:
        hm = HEAD.match(l)
        if hm:
            ids.append(int(hm.group(1)))
            continue
        mm = META.match(l)
        if mm:
            n = int(mm.group(1))
            rows.append(n)
            if not (200 <= n <= 400):
                fails.append("%s: row %d out of bounds" % (fn, n))
            jid = mm.group(2)
            if not jid.startswith("UNX-F%d-J1" % ids[-1] if ids else "UNX-F0"):
                fails.append("%s: judge id mismatch: %s" % (fn, jid))
            jids.append(jid)
    if len(ids) != 20:
        fails.append("%s: entry-count=%d" % (fn, len(ids)))
    if ids != list(range(f1, f2 + 1)):
        fails.append("%s: ID not %d–%d contiguous" % (fn, f1, f2))
    s = sum(rows)
    if s != 6000:
        fails.append("%s: rows-sum=%d != 6000" % (fn, s))
    if len(jids) != len(set(jids)):
        fails.append("%s: judge ids duplicated" % fn)
    all_ids += ids
    all_jids += jids
    total += s

if all_ids != list(range(17601, 17901)):
    fails.append("global ID not contiguous F17601–F17900 (n=%d)" % len(all_ids))
if len(set(all_jids)) != 300:
    fails.append("global judge ids not unique: %d" % len(set(all_jids)))
if total != 90000:
    fails.append("domain rows %d != 90000" % total)

# ⑥ 防重 grep 五范围：F17601–F17900 不得出现在其他域资产（kernel/docs/START/_attic/他域 deepen/总纲他域段）
pat = re.compile(r"UNX-F17[6-8]\d\d|UNX-F17900")
hit = []
for root, exts in [
    (os.path.join(REPO, "kernel", "varix", "src"), (".rs", ".md", ".py", ".c", ".h", ".json", ".toml")),
    (os.path.join(REPO, "docs", "START"), (".md",)),
    (os.path.join(REPO, "_attic"), (".md", ".py")),
]:
    if not os.path.isdir(root):
        continue
    for dp, _, fns in os.walk(root):
        for f in fns:
            if not f.endswith(exts):
                continue
            p = os.path.join(dp, f)
            try:
                t = open(p, encoding="utf-8", errors="ignore").read()
            except OSError:
                continue
            hit += [p + ":" + mm.group(0) for mm in pat.finditer(t)]

deepen = os.path.join(REPO, "docs", "unxreal", "deepen")
for f in os.listdir(deepen):
    if not f.endswith(".md") or f.startswith(("E3", "_")):
        continue
    p = os.path.join(deepen, f)
    t = open(p, encoding="utf-8", errors="ignore").read()
    hit += [p + ":" + mm.group(0) for mm in pat.finditer(t)]

zonggang = os.path.join(REPO, "docs", "Varix", "CoRun Varix STAR II · Unxreal",
                        "CoRun Varix STAR II · Unxreal · 总纲与施工书.md")
t = open(zonggang, encoding="utf-8", errors="ignore").read()
# 按 §7.3 域段切分：他域段内出现本域 ID 即撞号；段外（正文引用）仅登记为白名单观察
sec_re = re.compile(r"^#### 7\.3-([A-P]\d) ", re.M)
starts = [(mm.group(1), mm.start()) for mm in sec_re.finditer(t)]
for i, (dom, st) in enumerate(starts):
    if dom == "E3":
        continue
    seg = t[st: starts[i + 1][1] if i + 1 < len(starts) else len(t)]
    hit += ["总纲 §7.3-%s:%s" % (dom, mm.group(0)) for mm in pat.finditer(seg)]
if hit:
    fails.append("防重 grep 命中 %d 处（首 5）: %s" % (len(hit), hit[:5]))

print("=== UNX-E3 骨架立账校验器 ===")
print("books=%d ids=%d judges-unique=%d rows=%d" % (len(books), len(all_ids), len(set(all_jids)), total))
if fails:
    print("FAIL (%d):" % len(fails))
    for f in fails:
        print("  x " + str(f))
    sys.exit(1)
print("ALL PASS (7 assertions) exit=0")
