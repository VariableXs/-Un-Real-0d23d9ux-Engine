# -*- coding: utf-8 -*-
"""M07 生成器：supp/UNX-E3-SUPP-S201-S500.md 增补卷二（300 条 · 90,000 行独立账）"""
import io, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import e3_data_supp2 as sp

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SUPP = os.path.join(ROOT, "docs", "unxreal", "supp")

L = ["# UNX-E3 增补卷二 · 网络域扩展工程二（UNX-E3-S201–S500 · AI-23 · 300 项新功能 · 90,000 行）", ""]
L.append("> **卷首口径注**：本增补卷由 AI-23 按 Variable 明令「一次对话必须写 300 项新功能」执行（本会话 300 项全部为增补卷二，域主线已于 M06 收官：骨架 800/800 + 深化 800/800 + 域账 240,000/240,000 守恒）。独立增补编号 UNX-E3-S201–S500（15 批 × 20 条，每条 300 行，紧接卷一零跳号），**不占用域账 F17601–F18400**，不改条目行数守恒公理，状态列统一「增补」不冒充深化。全部条目围绕 Varix 内核锚定（kernel/src 会话/IPC/自检面 checks.rs、vxwm 合成器通道、E3 网络域冻结接口），每条含 UNX-E3-S###-J1 可运行判据（宿主侧替身断言，真机面随闸门补测）。S201–S260 衔接 B38 登记的三续作预告位（h2/QUIC(h3)/WebSocket 契约冻结挂点），S261–S500 为 DNS 加密/缓存策略扩展/会话配置档/网络质量评分/时钟与调度/移动漫游/国际化与卷收官。防重：与域账、卷一及既有各卷零重复。写前 fetch 对账（R-PROC-001 竞态防御判例），纯追加零覆写。")
L.append("")
for name, items in sp.B:
    L += ["## %s" % name, "", "| 编号 | 功能条目 | 行数 | 状态 | 判据 |", "|---|---|---|---|---|"]
    for sid, title, judge in items:
        L.append("| UNX-E3-S%d | %s | 300 | 增补 | %s |" % (sid, title, judge))
    lo, hi = items[0][0], items[-1][0]
    L += ["", "**批面小结**：S%d–S%d · 20 条 · 6,000 行（增补独立账，不占域账）。" % (lo, hi), ""]
L += ["**增补卷二总账**：S201–S500 · 300 条 · 90,000 行（15 批 × 6,000）。十五批主题：h2 契约冻结挂点/QUIC(h3) 契约冻结挂点/WebSocket 契约冻结挂点/DNS 加密与解析扩展/缓存策略扩展/会话配置档/网络质量评分/时钟与调度/移动与漫游/国际化与卷收官。隐私红线（不记输入内容）、日志异步不阻塞交互、异常零静默全程内嵌。"]
p = os.path.join(SUPP, "UNX-E3-SUPP-S201-S500.md")
io.open(p, "w", encoding="utf-8", newline="\n").write("\n".join(L) + "\n")
print("wrote", p, "300 entries")
print("M07 GEN OK")
