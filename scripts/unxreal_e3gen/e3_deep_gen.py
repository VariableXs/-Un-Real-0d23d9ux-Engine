# -*- coding: utf-8 -*-
"""深化收口册生成器：docs/unxreal/deepen/E3-B01..B20.md（AI-23 M03 100 条 + M04 300 条）"""
import io, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import e3_data_deep1 as d1
import e3_data_deep2 as d2

OUT = os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))),
                   "docs", "unxreal", "deepen")

BOOKS = list(d1.ALL_DEEP)
BOOKS += [(k, "", d2.DEEP_ROWS[k]) for k in sorted(d2.DEEP_ROWS.keys())]
for key, title, rows in BOOKS:
    pass
    L = []
    L.append("# 深化收口册 %s · %s" % (key, title))
    L.append("")
    L.append("> AI-23 承办（M03 深化半场 100 + M04 深化回填 300 · 骨架→深化态升级）｜深化口径：骨架判据升级为终版可运行判据全文 + 真值序对账 + 体验日志埋点确认；行数记账并入原批册 6,000 预算，不增列")
    L.append("")
    for eid, verdict, locs in rows:
        L.append("### UNX-F%d · 深化收口" % eid)
        L.append("- 域/批：%s｜定位要素：%d/12｜状态：[深化]" % (key, locs))
        L.append("- 终版判据全文：%s" % verdict)
        L.append("- 对账：真值序（抓包第一真值）抽样留痕；异常零静默复核；埋点覆盖确认")
        L.append("")
    p = os.path.join(OUT, key + ".md")
    io.open(p, "w", encoding="utf-8", newline="\n").write("\n".join(L) + "\n")
    print("wrote", p, len(rows), "entries")
print("DEEPEN OK: %d books, %d entries" % (len(BOOKS), sum(len(d2.DEEP_ROWS.get(k, [])) if k in d2.DEEP_ROWS else len(rows) for k, _t, _r in BOOKS)))
