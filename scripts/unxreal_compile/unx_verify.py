# -*- coding: utf-8 -*-
"""独立复核：汇编册 vs 源册 逐批一致性（条目数/行数/ID 集合）"""
import os, re

REPO = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
BATCH = os.path.join(REPO, "docs", "unxreal", "batches")
OUT = os.path.join(REPO, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")

SRC_HEAD = re.compile(r"^###\s+UNX-(F\d+)\s*·")
SRC_META = re.compile(r"^- 域/批：.*?纯功能行数：(\d+)")
SRC_HEAD_B = re.compile(r"^###\s+(F\d+)\s+")
SRC_META_B = re.compile(r"^- 域 UNX-[A-P]\d\s*·\s*批 B\d+\s*·\s*行数锁定\s*(\d+)")

COMP_H3 = re.compile(r"^####\s+(UNX-[A-P]\d-B\d+)\s*·")
COMP_ENTRY = re.compile(r"^\|\s*UNX-(F\d+)\s*\|")
COMP_HINT = re.compile(r"·\s*(\d+)\s*条）")


def parse_src():
    out = {}
    for fn in sorted(os.listdir(BATCH)):
        if not fn.startswith("UNX-") or not fn.endswith(".md"):
            continue
        text = open(os.path.join(BATCH, fn), encoding="utf-8").read()
        ids, rows, cur, hint = [], [], None, None
        for line in text.splitlines():
            s = line.strip()
            if not hint:
                m0 = COMP_HINT.search(s if s.startswith("####") or s.startswith("# ") else "")
            mh = SRC_HEAD.match(s) or SRC_HEAD_B.match(s)
            if mh:
                cur = mh.group(1) if mh.group(1).startswith("F") else mh.group(1)
                ids.append(cur)
                continue
            if cur is not None:
                mm = SRC_META.match(s) or SRC_META_B.match(s)
                if mm:
                    rows.append(int(mm.group(1)))
        out[fn[:-3]] = {"ids": ids, "rows": rows}
    return out


def parse_comp():
    out, order = {}, []
    cur = None
    for line in open(OUT, encoding="utf-8"):
        s = line.rstrip("\n")
        m = COMP_H3.match(s)
        if m:
            cur = m.group(1)
            out[cur] = {"ids": [], "rows": []}
            order.append(cur)
            continue
        if cur:
            me = COMP_ENTRY.match(s)
            if me:
                out[cur]["ids"].append(me.group(1))
                # 转义竖线 \| 不参与分列：先整体切分再还原
                parts = re.split(r"(?<!\\)\|", s)
                rv = parts[3].strip() if len(parts) > 3 else ""
                out[cur]["rows"].append(int(rv) if rv.isdigit() else 0)
    return out, order


src = parse_src()
comp, order = parse_comp()

print("src books :", len(src))
print("comp books:", len(comp))
print("missing   :", [b for b in src if b not in comp])
print("extra     :", [b for b in comp if b not in src])

bad = []
for b in order:
    if b not in src:
        continue
    s, c = src[b], comp[b]
    if s["ids"] != c["ids"]:
        bad.append((b, "ID-SET", len(s["ids"]), len(c["ids"]),
                    [x for x in s["ids"] if x not in c["ids"]][:3]))
    elif sum(s["rows"]) != sum(c["rows"]):
        bad.append((b, "ROWS", sum(s["rows"]), sum(c["rows"]), ""))

t_src = sum(sum(v["rows"]) for v in src.values())
t_cmp = sum(sum(v["rows"]) for v in comp.values())
t_src_ids = sum(len(v["ids"]) for v in src.values())
t_cmp_ids = sum(len(v["ids"]) for v in comp.values())
print("")
print("TOTAL entries src {:,} | comp {:,} | equal {}".format(t_src_ids, t_cmp_ids, t_src_ids == t_cmp_ids))
print("TOTAL rows    src {:,} | comp {:,} | equal {}".format(t_src, t_cmp, t_src == t_cmp))
print("mismatched batches:", len(bad))
for x in bad[:20]:
    print("  !", x)
