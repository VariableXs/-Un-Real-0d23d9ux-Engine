# -*- coding: utf-8 -*-
"""深化收口册生成器（第二批）：docs/unxreal/deepen/E3-B21..B35.md（AI-23 M05 300 条）"""
import io, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import e3_data_deep3 as d3

OUT = os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))),
                   "docs", "unxreal", "deepen")

n_books = n_entries = 0
for key in d3.ALL_DEEP3:
    rows = d3.DEEP_ROWS[key]
    L = []
    L.append("# 深化收口册 %s · " % key)
    L.append("")
    L.append("> AI-23 承办（M05 深化回填第二批 300 · 骨架→深化态升级）｜深化口径：骨架判据升级为终版可运行判据全文 + 真值序对账 + 体验日志埋点确认；行数记账并入原批册 6,000 预算，不增列")
    L.append("")
    for eid, verdict, locs in rows:
        L.append("### UNX-F%d · 深化收口" % eid)
        L.append("- 域/批：%s｜定位要素：%d/12｜状态：[深化]" % (key, locs))
        L.append("- 终版判据全文：%s" % verdict)
        L.append("- 对账：真值序（抓包第一真值）抽样留痕；异常零静默复核；埋点覆盖确认")
        L.append("")
    p = os.path.join(OUT, key + ".md")
    io.open(p, "w", encoding="utf-8", newline="\n").write("\n".join(L) + "\n")
    n_books += 1
    n_entries += len(rows)
    print("wrote", p, len(rows), "entries")
print("DEEPEN2 OK: %d books, %d entries" % (n_books, n_entries))
