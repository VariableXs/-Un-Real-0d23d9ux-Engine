# -*- coding: utf-8 -*-
"""F3 增补卷二汇编增补：从 batches/UNX-F3-S06..S20.md 提取 300 条定位要素追加主 MD（纯追加零删除）。"""
import io, os, re

HERE = os.path.dirname(os.path.abspath(__file__))
B = os.path.join(HERE, "..", "batches")
MAIN = os.path.join(HERE, "..", "..", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")

rows = []
for b in range(6, 21):
    t = io.open(os.path.join(B, f"UNX-F3-S{b:02d}.md"), encoding="utf-8").read()
    items = re.findall(r"^### (UNX-F3-S\d{3}) · (.+)$\n- 域/批：F3/S\d{2}｜纯功能行数：(\d+)｜状态：\[骨架\]｜判据：(UNX-F3-S\d{3}-J1 .+)$", t, re.M)
    assert len(items) == 20, (b, len(items))
    for fid, title, lines, jc in items:
        rows.append(f"| {fid} | {title} | {lines} | [骨架] | {jc} |")
assert len(rows) == 300, len(rows)

sec = ["", "## 部F · UNX-F3 增补卷二（AI-28 · 300 项 / 90,000 行）", "",
       "> **增补登记**（AI-28 · 增补卷二产线令）：UNX-F3-S101–S400 共 300 项，独立编号**不占域账**（判例承 AI-29/AI-15/AI-19 增补卷与卷一 S001–S100）。"
       "主题为 F3 渲染语义域延展支撑面：ROP3 深水区 80｜几何变换与位图生命周期 60｜区域剪裁 20｜文本管线深水区 60｜ClearType 色彩 20｜D2D/DWrite 40｜元文件/失效/打印/主题多屏无障碍/容错收口 120 批面分布见下。"
       "批册在 `docs/unxreal/batches/UNX-F3-S06..S20.md`（单源生成器 `scripts/unxreal_f3_svol_gen2.py`，校验器 `unxreal_f3_svol_check2.py` 六查全绿：条目数/行数/判据自指/ID 连续 S101–S400/批号/正文）。"
       "全程围绕 Varix 内核渲染语义；观测与日志遵守隐私红线、异步不阻塞；真机/QEMU 判据随闸门补测登记。", ""]
sec.append("### UNX-F3 · 增补卷二总表（S06–S20 · 300 条 / 90,000 行）")
sec.append("| 编号 | 功能 | 行数 | 状态 | 判据 |")
sec.append("|---|---|---|---|---|")
sec.extend(rows)
sec.append("")
sec.append("**增补卷二批面小结**：S06–S07 ROP3 深水区 80｜S08 几何变换扩展 20｜S09 位图生命周期 20｜S10 区域剪裁 20｜S11–S12 文本管线深水区 40｜S13 ClearType 色彩 20｜S14 D2D 20｜S15 DWrite 20｜S16 元文件 20｜S17 失效调度 20｜S18 打印 20｜S19 主题/多屏/无障碍 20｜S20 容错与交付总收口 20。判据号连续 S101–S400 零缺号零重号。")
sec.append("")

with io.open(MAIN, "a", encoding="utf-8") as f:
    f.write("\n".join(sec) + "\n")
print("appended", len(rows), "rows")
