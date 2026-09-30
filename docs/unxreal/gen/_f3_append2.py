# -*- coding: utf-8 -*-
import re, io
rows = []
total_rows = 0
for b in range(16, 31):
    txt = io.open("batches/UNX-F3-B%02d.md" % b, encoding="utf-8").read()
    pat = (r"### (UNX-F\d+) · (.+)\n- 域/批：F3/B(\d+)｜纯功能行数：(\d+)｜状态：\[(.+?)\]｜判据：(.+)")
    items = re.findall(pat, txt)
    assert len(items) == 20, (b, len(items))
    for fid, name, _b, lines, status, jc in items:
        total_rows += int(lines)
        rows.append("| %s | %s | %s | %s | %s |" % (fid, name, lines, status, jc))
assert len(rows) == 300 and total_rows == 90000, (len(rows), total_rows)
# 判据号自检：判据文本内的编号必须与条目 ID 一致
for r in rows:
    fid = r.split("|")[1].strip()
    jc = r.split("|")[-2].strip()
    m = re.match(r"^(UNX-F\d+)-J1", jc)
    assert m and m.group(1) == fid, (fid, jc[:40])

sec = []
sec.append("")
sec.append("### UNX-F3 · 第二产段（B16–B30 · 300 条 / 90,000 行 · F21901–F22200）")
sec.append("")
sec.append("> **增补登记**（AI-28 · 第二次会话产线令 300 项）：F3 域第二产段 15 批 B16–B30 共 300 条 / 90,000 行，正文与判据在 `docs/unxreal/batches/UNX-F3-B16..B30.md`。分段：位块/元文件深化 80（B16–B19）｜色彩管理与亚像素深化 40（B20）｜D2D 深化 60（B21–B22）｜DWrite 深化 20（B23）｜指令流/失效区/打印/主题支撑/容错/无障碍支撑/多屏/收官 100（B24–B30）。累计域账 600/800 条 · 180,000/240,000 行。")
sec.append("")
sec.append("| 编号 | 功能 | 行数 | 状态 | 判据 |")
sec.append("|---|---|---|---|---|")
sec.extend(rows)
sec.append("")

out = "\n".join(sec) + "\n"
path = "../Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md"
with io.open(path, "a", encoding="utf-8") as f:
    f.write(out)
print("OK rows=", len(rows), "lines=", total_rows)
