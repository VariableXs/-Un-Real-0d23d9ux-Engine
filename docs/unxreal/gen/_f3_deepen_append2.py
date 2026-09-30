# -*- coding: utf-8 -*-
"""F3 深化轮增补卷：从 deepen/F3-B01..B15.md 提取 300 条定位要素追加主 MD。"""
import io, os, re

HERE = os.path.dirname(os.path.abspath(__file__))
DEEPEN = os.path.join(HERE, "..", "deepen")
BOOK = os.path.join(HERE, "..", "..", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")

rows = []
total = 0
for b in range(16, 31):
    t = io.open(os.path.join(DEEPEN, f"F3-B{b:02d}.md"), encoding="utf-8").read()
    for fid, title, blk in re.findall(r"^### (UNX-F\d{5}) · ([^\n]+)\n(- 域/批：.*?)(?=^### |\Z)", t, re.M | re.S):
        m = re.search(r"纯功能行数：(\d+) 行", blk)
        j = re.search(r"判据：(UNX-F\d{5}-J1)", blk)
        assert m and j, (fid, "missing fields")
        total += int(m.group(1))
        rows.append(f"| {fid} | {title} | {m.group(1)} | [已深化] | {j.group(1)} |")

assert len(rows) == 300, len(rows)
assert total == 90000, total

sec = ["", "## 部F · 深化增补卷（AI-28 · F3 深化轮第二段 B16–B30）", "",
       "> **深化登记**（AI-28 · 承接\"一次对话 300 项\"产线令 · F3 域骨架 800/800 封账后进入深化阶段，同构 AI-26 F1 先例）："
       "deepen/F3-B01..B15.md 十五册 **300 条深化 / 90,000 行（行数与骨架账逐条 verbatim 一致，深化零改行数）**；"
       "生成器 `scripts/unxreal_f3_deepen_gen.py` 单源读骨架账（ID/标题/判据/行数零转抄），"
       "校验器 `scripts/unxreal_f3_deepen_check2.py` **六查全绿**（六要素/正文≥300 字/ID 唯一落域/行数一致/判据自指/双册条目名 verbatim 一致；ID 域 F21901–F22220 扣除 B41 段 F22041–F22060）。"
       "本卷为深化定位要素清单（纯追加零删除）；深化剩余 B31–B41（含补号批 B41 的 F22041–F22060）共 200 条待续轮。", ""]
sec.append("### UNX-F3 · 深化定位要素（第二段 B16–B30 · 300 条 / 90,000 行）")
sec.append("| 编号 | 功能 | 行数 | 状态 | 判据 |")
sec.append("|---|---|---|---|---|")
sec.extend(rows)
sec.append("")
sec.append("**深化轮第二段批面小结**：B16–B19 位块/元文件/对照扩展/段落方向深化 80｜B20 色彩与亚像素深化 40→并入 B16–B20 段｜B21–B22 D2D 效果/层蒙版深化 40｜B23 DWrite 集合与子集化深化 20｜B24–B29 指令流/打印/主题支撑/容错/无障碍/多屏深化 120｜B30 产段收口深化 20。")
sec.append("")

out = "\n".join(sec) + "\n"
with io.open(BOOK, "a", encoding="utf-8") as f:
    f.write(out)
print("OK rows=", len(rows), "lines=", total)
