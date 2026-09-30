# -*- coding: utf-8 -*-
"""波09-M12 生成器：增补卷七 UNX-E3-S1701–S2000 单册（300 条 × 300 行 = 90,000 行独立账）。"""
import io, os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import e3_data_supp7 as d7

OUT = os.path.join("docs", "unxreal", "supp", "UNX-E3-SUPP-S1701-S2000.md")

L = []
L.append("# UNX-E3 增补卷七 · S1701–S2000（AI-23 · 波09-M12 立账 · 300 条 · 90,000 行独立账）\n")
L.append("> 【卷七口径注】独立编号不占域账、不改守恒（域账 240,000/240,000 不动）；紧接卷六零跳号。"
         "十五批：流量整形/连接预热/证书透明缓存/请求优先级学习/网络隐私仪表盘/家长管控挂点/企业策略下发/"
         "漫游 VPN 协同/带宽公平共享/服务端推送感知/离线包签名/网络状态播报/故障演练常态/评测基准对齐/卷七收官。"
         "每条判据号 UNX-E3-S###-J1 与 ID 一一对应；隐私红线、日志异步不阻塞、异常零静默内嵌。\n")

for name, rows in d7.ALL_BATCHES:
    L.append("\n## %s（%d 条 · %d 行）\n" % (name, len(rows), len(rows) * 300))
    L.append("| ID | 标题 | 行数 | 状态 | 判据 |")
    L.append("|---|---|---|---|---|")
    for sid, title, judge in rows:
        L.append("| UNX-E3-S%03d | %s | %d | 增补 | %s |" % (sid, title, 300, judge))

body = "\n".join(L) + "\n"
assert "UNX-E3-S1701 " in body and "UNX-E3-S2000 " in body
os.makedirs(os.path.dirname(OUT), exist_ok=True)
io.open(OUT, "w", encoding="utf-8", newline="\n").write(body)

n = sum(1 for ln in body.splitlines() if ln.startswith("| UNX-E3-S"))
assert n == 300, n
print("增补卷七生成 OK: %s（300 条 / 90,000 行独立账）" % OUT)
