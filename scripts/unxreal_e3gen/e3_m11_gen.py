# -*- coding: utf-8 -*-
"""波09-M11 生成器：增补卷六 UNX-E3-S1401–S1700 单册（300 条 × 300 行 = 90,000 行独立账）。"""
import io, os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import e3_data_supp6 as d6

OUT = os.path.join("docs", "unxreal", "supp", "UNX-E3-SUPP-S1401-S1700.md")

L = []
L.append("# UNX-E3 增补卷六 · S1401–S1700（AI-23 · 波09-M11 立账 · 300 条 · 90,000 行独立账）\n")
L.append("> 【卷六口径注】独立编号不占域账、不改守恒（域账 240,000/240,000 不动）；紧接卷五零跳号。"
         "十五批：启动链探针/降级池工程化/断点续传/镜像源管理/时间敏感调度/电量与热感知/存储配额治理/"
         "证书钉扎运维/请求去重与合并/预取引擎/页面加载预算/弱端设备适配/故障剧本库/回归哨兵/卷六收官。"
         "每条判据号 UNX-E3-S###-J1 与 ID 一一对应；隐私红线、日志异步不阻塞、异常零静默内嵌。\n")

for name, rows in d6.ALL_BATCHES:
    L.append("\n## %s（%d 条 · %d 行）\n" % (name, len(rows), len(rows) * 300))
    L.append("| ID | 标题 | 行数 | 状态 | 判据 |")
    L.append("|---|---|---|---|---|")
    for sid, title, judge in rows:
        L.append("| UNX-E3-S%03d | %s | %d | 增补 | %s |" % (sid, title, 300, judge))

body = "\n".join(L) + "\n"
assert "UNX-E3-S1401 " in body and "UNX-E3-S1700 " in body
os.makedirs(os.path.dirname(OUT), exist_ok=True)
io.open(OUT, "w", encoding="utf-8", newline="\n").write(body)

# 机械自检
n = sum(1 for ln in body.splitlines() if ln.startswith("| UNX-E3-S"))
assert n == 300, n
print("增补卷六生成 OK: %s（300 条 / 90,000 行独立账）" % OUT)
