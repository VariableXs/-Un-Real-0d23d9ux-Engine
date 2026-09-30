# -*- coding: utf-8 -*-
"""F3 收官段（B31–B40）汇编生成：从批册提取 200 行总表，自检后追加主 MD。"""
import io, re

rows = []
total = 0
for b in list(range(31, 41)) + [41]:
    txt = io.open(f"batches/UNX-F3-B{b}.md", encoding="utf-8").read()
    pat = re.compile(
        r"### (UNX-F\d+) · .+\n- 域/批：F3/B(\d+)｜纯功能行数：(\d+)｜状态：\[(.+?)\]｜判据：(UNX-F\d+-J1 .+)")
    items = pat.findall(txt)
    assert len(items) in (10, 20), (b, len(items))
    for fid, bn, lines, status, jc in items:
        assert int(bn) == b, (fid, bn, b)
        assert jc.startswith(fid + "-J1 "), (fid, jc[:40])  # 判据号与条目 ID 一致
        total += int(lines)
        rows.append(f"| {fid} | | {lines} | [{status}] | {jc} |")

assert len(rows) == 200, len(rows)
assert total == 60000, total

# 全 40 批 800 条唯一性
ids = []
for b in range(1, 42):
    p = f"batches/UNX-F3-B{b:02d}.md"
    ids += re.findall(r"^### (UNX-F\d+)", io.open(p, encoding="utf-8").read(), re.M)
assert len(ids) == 800 and len(set(ids)) == 800, (len(ids), len(set(ids)))

sec = ["", "## 部F · 图形渲染域（AI-28 收官段增补 · 域封账）", "",
       "> **增补登记**（AI-28 · 收官段 B31–B41）：200 条 / 60,000 行，域 41 批合计 **800 条（F21601–F22400 零缺号零重号全连续）/ 240,000 行守恒，F3 域骨架封账**。"
       "**编号纪律偏差登记（诚实账）**：第二产段曾跳号 F22021–F22040（B21 直跳 B22），本轮以 B22 整批改号 -20 + B41 补号批（渲染调试与开发者体验支撑面 20 条）修复为全连续；B39/B40 按余量拆为各 10 条。"
       "**差额说明**：本对话产线令 300 项，F3 域 ID 空间已满（800/800），本轮域内新增 200 项为上限；其余 100 项因 UNX-J3 归属两册矛盾（总纲\"产线归属 AI-28\" vs 分工图\"AI-48 承包\"）待 Variable 裁定后于下轮补足，未越界抢做。", ""]
sec.append("### UNX-F3 · 收官段（B31–B41 · 200 条 / 60,000 行）")
sec.append("| 编号 | 功能 | 行数 | 状态 | 判据 |")
sec.append("|---|---|---|---|---|")
sec.extend(rows)
sec.append("")
sec.append("**收官段批面小结**：B31 位块/元文件/指令流总收口 20｜B32–B33 文本对照与管线总装 40｜B34 ClearType 开关总闸 20｜B35–B36 D2D 总闸 40｜B37 DWrite 总闸 20｜B38 联签闭账 20｜B39–B40 域封账 20｜B41 补号批（调试与开发者体验）20。")
sec.append("")

out = "\n".join(sec) + "\n"
with io.open("../Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md", "a", encoding="utf-8") as f:
    f.write(out)
print("OK rows=", len(rows), "lines=", total, "domain_ids=", len(ids))
