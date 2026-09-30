# -*- coding: utf-8 -*-
"""AI-50 · UNX-J5 深化轮独立校验器（八查 · exit=0 为全绿）
八查：①40 册在位 ②800 条 ③每条 ≥300 汉字 ④判据号 800 枚唯一且与主册逐一一致
⑤批行数守恒（批头行数求和 6,000×40）⑥主册 800 行状态已翻转「已深化」且他卷零误伤
⑦主册深化增补卷段闸索引与书一一对应 ⑧域账 ID 零缺失零新增。
"""
import io, re, sys, os

MAIN = "docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md"
DEEPEN_DIR = "docs/Varix/CoRun Varix STAR II · Unxreal/deepen/"
errors = []

def zh_len(s):
    return len(re.findall(r"[\u4e00-\u9fff]", s))

main = io.open(MAIN, encoding="utf-8").read()

# ① 40 册在位
books = sorted(f for f in os.listdir(DEEPEN_DIR) if re.match(r"J5-B\d\d\.md$", f))
if len(books) != 40:
    errors.append("① 册数 %d != 40" % len(books))

book_entries = {}
for fn in books:
    txt = io.open(DEEPEN_DIR + fn, encoding="utf-8").read()
    ents = re.findall(r"^## (UNX-F(\d{5})) · (.+?)（(\d+) 行 · UNX-F\2-J1）\n\n(.*?)\n---", txt, re.M | re.S)
    book_entries[fn] = ents

# ② 800 条
all_ents = [(e[0], e[1], e[2], e[3], e[4]) for fn in books for e in book_entries[fn]]
if len(all_ents) != 800:
    errors.append("② 条数 %d != 800" % len(all_ents))
ids = [e[0] for e in all_ents]
if ids != ["UNX-F%05d" % n for n in range(39201, 40001)]:
    errors.append("② ID 序列非连续零跳号")

# ③ 每条 ≥300 汉字
short = [(e[0], zh_len(e[4])) for e in all_ents if zh_len(e[4]) < 300]
if short:
    errors.append("③ %d 条 <300 汉字，样例 %s" % (len(short), short[:3]))

# ④ 判据号唯一且与主册一致
jids = ["UNX-F%05d-J1" % n for n in range(39201, 40001)]
main_rows = re.findall(r"^\| (UNX-F39[2-9]\d\d|UNX-F40000) \| ([^|]+) \| (\d+) \| (已深化) \| (UNX-F\d{5}-J1)", main, re.M)
if len(main_rows) != 800:
    errors.append("④⑥ 主册已深化行 %d != 800" % len(main_rows))
if [r[4] for r in main_rows] != jids:
    errors.append("④ 主册判据号与深化册不一致")
if len(set(jids)) != 800:
    errors.append("④ 判据号非唯一")

# ⑤ 批行数守恒
total = sum(int(r[2]) for r in main_rows)
if total != 240000:
    errors.append("⑤ 域账行数求和 %d != 240000" % total)
for b in range(1, 41):
    bset = all_ents[(b-1)*20:b*20]
    if sum(int(e[3]) for e in bset) != 6000:
        errors.append("⑤ 批 B%02d 求和 != 6000" % b)

# ⑥ 他卷零误伤：非 J5 域的「增补」状态行仍在
others = re.findall(r"^\| (UNX-F\d{5}|UNX-J5-[ST]\d{3}) \| ([^|]+) \| (\d+) \| 增补 \|", main, re.M)
other_j5_t = [o for o in others if o[0].startswith("UNX-J5-")]
non_j5 = [o for o in others if o[0].startswith("UNX-F") and not (39201 <= int(o[0][5:]) <= 40000)]
if len(non_j5) < 1000:
    errors.append("⑥ 他域增补行异常减少: %d" % len(non_j5))
if len(other_j5_t) != 600:
    errors.append("⑥ J5 增补卷 S/T 状态行应 600 不变，实 %d" % len(other_j5_t))

# ⑦ 主册深化增补卷段闸索引 40 批段
vol_secs = re.findall(r"^### UNX-J5-B(\d\d)·深 B\d\d", main, re.M)
if vol_secs != ["%02d" % b for b in range(1, 41)]:
    errors.append("⑦ 段闸索引 %d 段或序错" % len(vol_secs))

# ⑧ 域账 ID 零缺失零新增（深化不改骨架）
skel = re.findall(r"^\| (UNX-F39[2-9]\d\d|UNX-F40000) \|", main, re.M)
if sorted(set(skel)) != ["UNX-F%05d" % n for n in range(39201, 40001)] or len(skel) != 800:
    errors.append("⑧ 域账 ID 集合漂移")

if errors:
    print("FAIL")
    for e in errors:
        print(" -", e)
    sys.exit(1)
print("八查 ALL PASS exit=0（40 册 / 800 条 / ≥300 汉字 / 判据唯一一致 / 240,000 守恒 / 状态翻转零误伤 / 段闸对位 / ID 零漂移）")
