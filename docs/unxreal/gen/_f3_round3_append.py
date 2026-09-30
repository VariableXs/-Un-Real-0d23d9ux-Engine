# -*- coding: utf-8 -*-
"""F3 第三轮汇编增补：深化第三段 200 行 + 增补卷一 100 行追加主 MD。纯追加零删除。"""
import io, os, re

HERE = os.path.dirname(os.path.abspath(__file__))
UNX = os.path.join(HERE, "..")
BOOK = os.path.join(HERE, "..", "..", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")

# --- 深化第三段（deepen/F3-B31..B41）---
d_rows, d_total = [], 0
for b in range(31, 42):
    t = io.open(os.path.join(UNX, "deepen", f"F3-B{b:02d}.md"), encoding="utf-8").read()
    for fid, title, blk in re.findall(r"^### (UNX-F\d{5}) · ([^\n]+)\n(- 域/批：.*?)(?=^### |\Z)", t, re.M | re.S):
        m = re.search(r"纯功能行数：(\d+) 行", blk)
        j = re.search(r"判据：(UNX-F\d{5}-J1)", blk)
        assert m and j, (fid, "missing")
        d_total += int(m.group(1))
        d_rows.append(f"| {fid} | {title} | {m.group(1)} | [已深化] | {j.group(1)} |")
assert len(d_rows) == 200 and d_total == 60000, (len(d_rows), d_total)

# --- 增补卷一（batches/UNX-F3-S01..S05）---
s_rows = []
for b in range(1, 6):
    t = io.open(os.path.join(UNX, "batches", f"UNX-F3-S{b:02d}.md"), encoding="utf-8").read()
    for fid, title, blk in re.findall(r"^### (UNX-F3-S\d{3}) · ([^\n]+)\n(- 域/批：.*?)(?=^### |\Z)", t, re.M | re.S):
        j = re.search(r"判据：(UNX-F3-S\d{3}-J1 [^｜\n]{0,40})", blk)
        assert j, (fid, "no judge")
        s_rows.append(f"| {fid} | {title} | 300 | [骨架] | {j.group(1).strip()}… |")
assert len(s_rows) == 100, len(s_rows)

sec = ["", "## 部F · 深化收官与增补卷一（AI-28 · F3 域深化满账 + 观测对照增补）", "",
       "> **登记**（AI-28 · 承接\"一次对话 300 项\"产线令 · 本轮 = 深化 200 + 增补卷一 100）：",
       "① **深化轮第三段** deepen/F3-B31..B41.md 十一册 **200 条深化 / 60,000 行**（行数与骨架 verbatim 一致，校验器 `scripts/unxreal_f3_deepen_check3.py` 六查全绿 exit=0）——至此 **F3 域深化 800/800 满账收官**（骨架 800/800 + 深化 800/800，域账 240,000 行双重守恒）；",
       "② **增补卷一** batches/UNX-F3-S01..S05.md 五册 **100 项新功能 / 30,000 行**，独立编号 UNX-F3-S001–S100 **不占域账**（判例承 AI-29/AI-15/AI-19 增补卷），主题：渲染观测回放 40｜对照工程扩展 40｜性能遥测与交付收口 20；生成器 `scripts/unxreal_f3_svol_gen.py` 单源、零重号、J1 判据 100/100、与域账 F 区零撞号。", ""]
sec.append("### UNX-F3 · 深化定位要素（第三段 B31–B41 · 200 条 / 60,000 行 · 域深化满账）")
sec.append("| 编号 | 功能 | 行数 | 状态 | 判据 |")
sec.append("|---|---|---|---|---|")
sec.extend(d_rows)
sec.append("")
sec.append("**深化第三段批面小结**：B31 位块/元文件/指令流收口深化 20｜B32–B33 文本对照与管线总装深化 40｜B34 ClearType 开关总闸深化 20｜B35–B36 D2D 几何/效果/资源生命周期深化 40｜B37 DWrite 字体系统总闸深化 20｜B38 跨域联签深化 20｜B39–B40 域总收官账本深化 20｜B41 补号批（渲染调试与开发者体验）深化 20。**域深化累计 800/800（240,000/240,000），F3 域骨架+深化双满账收官。**")
sec.append("")
sec.append("### UNX-F3 · 增补卷一（S001–S100 · 100 项 / 30,000 行 · 独立编号不占域账）")
sec.append("| 编号 | 功能 | 行数 | 状态 | 判据 |")
sec.append("|---|---|---|---|---|")
sec.extend(s_rows)
sec.append("")
sec.append("**增补卷一批面小结**：S01–S02 渲染观测回放 40｜S03–S04 对照工程扩展 40｜S05 性能遥测与域交付收口 20。观测面全程遵守隐私红线（不记输入内容）、日志异步不阻塞交互。")
sec.append("")

with io.open(BOOK, "a", encoding="utf-8") as f:
    f.write("\n".join(sec) + "\n")
print("OK deepen3 rows=", len(d_rows), "svol rows=", len(s_rows))
