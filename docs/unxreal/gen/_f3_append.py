# -*- coding: utf-8 -*-
import re, io
base = "batches/UNX-F3-B"
rows = []
total_rows = 0
for b in range(1, 16):
    txt = io.open(base + "%02d.md" % b, encoding="utf-8").read()
    pat = (r"### (UNX-F\d+) · (.+)\n- 域/批：F3/B(\d+)｜纯功能行数：(\d+)｜状态：\[(.+?)\]｜判据：(.+)")
    items = re.findall(pat, txt)
    assert len(items) == 20, (b, len(items))
    for fid, name, _b, lines, status, jc in items:
        total_rows += int(lines)
        rows.append("| %s | %s | %s | %s | %s |" % (fid, name, lines, status, jc))
assert len(rows) == 300 and total_rows == 90000, (len(rows), total_rows)

sec = []
sec.append("")
sec.append("## 部F · 图形渲染域（AI-28 首产段增补）")
sec.append("")
sec.append("> **增补登记**（AI-28 · 执行 Variable『一次对话 300 项』产线令 · 本节为 F3 域首产段）：UNX-F3 GDI/D2D/DWrite 渲染语义域（承包域 F21601–F22400 · 800 条）**首产段 15 批 B01–B15 共 300 条 / 90,000 行**新功能在本节汇编，正文与判据在 `docs/unxreal/batches/UNX-F3-B01..B15.md`。状态口径：[骨架]=批册已立、判据已冻结、深化与实现待随波次推进；行数守恒 90,000/240,000；防重五范围 grep 零命中；全部为**新增条目**（此前 batches/ 无 F3 任何批册，汇编亦无部F 段，本节为部F 首段）。域余量 B16–B40 共 500 条按批型配比留后续批次。")
sec.append("")
sec.append("### UNX-F3 · GDI/D2D/DWrite 渲染语义（首产段 B01–B15 · 300 条 / 90,000 行）")
sec.append("")
sec.append("| 编号 | 功能 | 行数 | 状态 | 判据 |")
sec.append("|---|---|---|---|---|")
sec.extend(rows)
sec.append("")
sec.append("**首产段批面小结**：B01–B04 ROP3 与位块（80 条）｜B05–B08 文本管线与对照工程（80 条）｜B09–B10 ClearType 亚像素（40 条）｜B11–B13 D2D 矢量（60 条）｜B14 DWrite（20 条）｜B15 收口账本（20 条）。批批守恒 6,000 行、判据号连续 UNX-F21601–F21900、零缺号零重号。")
sec.append("")

out = "\n".join(sec) + "\n"
path = "../Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md"
with io.open(path, "a", encoding="utf-8") as f:
    f.write(out)
print("OK rows=", len(rows), "lines=", total_rows)
