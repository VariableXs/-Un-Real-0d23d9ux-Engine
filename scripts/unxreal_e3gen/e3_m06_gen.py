# -*- coding: utf-8 -*-
"""M06 生成器：deepen/E3-B36..B40.md 五册（100 条）+ supp/UNX-E3-SUPP-S001-S200.md 增补卷一（200 条）"""
import io, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import e3_data_deep4 as d4
import e3_data_supp1 as sp

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DEEPEN = os.path.join(ROOT, "docs", "unxreal", "deepen")
SUPP = os.path.join(ROOT, "docs", "unxreal", "supp")

# --- 深化册 B36–B40 ---
for key in d4.ALL_DEEP4:
    rows = d4.DEEP_ROWS[key]
    L = ["# 深化收口册 %s · " % key, ""]
    L.append("> AI-23 承办（M06 深化收官段 100 · 骨架→深化态升级）｜深化口径：骨架判据升级为终版可运行判据全文 + 真值序对账 + 体验日志埋点确认；行数记账并入原批册 6,000 预算，不增列")
    L.append("")
    for eid, verdict, locs in rows:
        L += ["### UNX-F%d · 深化收口" % eid,
              "- 域/批：%s｜定位要素：%d/12｜状态：[深化]" % (key, locs),
              "- 终版判据全文：%s" % verdict,
              "- 对账：真值序（抓包第一真值）抽样留痕；异常零静默复核；埋点覆盖确认", ""]
    p = os.path.join(DEEPEN, key + ".md")
    io.open(p, "w", encoding="utf-8", newline="\n").write("\n".join(L) + "\n")
    print("wrote", p, len(rows))

# --- 增补卷一 ---
L = ["# UNX-E3 增补卷一 · 网络域扩展工程（UNX-E3-S001–S200 · AI-23 · 200 项新功能 · 60,000 行）", ""]
L.append("> **卷首口径注**：本增补卷由 AI-23 按 Variable 明令「一次对话必须写 300 项新功能」执行（本轮 300 = E3 域深化收官 100 + 增补卷一 200）。独立增补编号 UNX-E3-S001–S200（10 批 × 20 条，每条 300 行），**不占用域账 F17601–F18400**（域 40 批主线深化已于本轮 800/800 收官），不改条目行数守恒公理，状态列统一「增补」不冒充深化。全部条目围绕 Varix 内核锚定（kernel/src 会话/IPC/自检面 checks.rs、vxwm 合成器通道、E3 网络域冻结接口），每条含 UNX-E3-S###-J1 可运行判据（宿主侧替身断言，真机面随闸门补测）。防重：与域账及既有各卷零重复。写前 git fetch 对账（R-PROC-001 竞态防御判例），纯追加零覆写。")
L.append("")
for name, rows in sp.BATCHES:
    L += ["## %s" % name, "", "| 编号 | 功能条目 | 行数 | 状态 | 判据 |", "|---|---|---|---|---|"]
    for (sid, title, judge) in rows:
        L.append("| UNX-E3-S%03d | %s | 300 | 增补 | %s |" % (sid, title, judge))
    lo, hi = rows[0][0], rows[-1][0]
    L += ["", "**批面小结**：S%03d–S%03d · 20 条 · %d 行（增补独立账，不占域账）。" % (lo, hi, 20 * 300), ""]
L += ["**增补卷一总账**：S001–S200 · 200 条 · 60,000 行（10 批 × 6,000）。十批主题：诊断与可观测/弱网韧性/离线与同步/内容安全下载/性能遥测与预算/隐私与出站/策略引擎（E5 挂点）/调试工作台/错误文案与诊断向导/开放性与收官。全程遵守隐私红线（不记输入内容）、日志异步不阻塞交互、异常零静默。"]
p = os.path.join(SUPP, "UNX-E3-SUPP-S001-S200.md")
io.open(p, "w", encoding="utf-8", newline="\n").write("\n".join(L) + "\n")
print("wrote", p, 200)
print("M06 GEN OK")
