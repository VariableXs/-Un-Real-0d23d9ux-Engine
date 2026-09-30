# -*- coding: utf-8 -*-
"""UNX-D2 B01-B40 骨架断言链机械校验器（AI-17 会话 · 域满账扩表版）
校验：批数/每批条数/ID 连续/行数求和=批头登记/域累计/判据编号格式/状态合法。
域满账断言：40 批 / 800 条 / F12801-F13600 无重复无空洞 / 域累计 240,000。
"""
import re, sys, io
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")

BASE = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/docs/unxreal/batches"
BATCHES = [
    ("UNX-D2-B01.md", "F12801", "F12820", 5800),
    ("UNX-D2-B02.md", "F12821", "F12840", 5920),
    ("UNX-D2-B03.md", "F12841", "F12860", 5540),
    ("UNX-D2-B04.md", "F12861", "F12880", 5520),
    ("UNX-D2-B05.md", "F12881", "F12900", 5680),
    ("UNX-D2-B06.md", "F12901", "F12920", 5300),
    ("UNX-D2-B07.md", "F12921", "F12940", 5360),
    ("UNX-D2-B08.md", "F12941", "F12960", 5400),
    ("UNX-D2-B09.md", "F12961", "F12980", 5580),
    ("UNX-D2-B10.md", "F12981", "F13000", 5320),
    ("UNX-D2-B11.md", "F13001", "F13020", 6360),
    ("UNX-D2-B12.md", "F13021", "F13040", 5300),
    ("UNX-D2-B13.md", "F13041", "F13060", 5700),
    ("UNX-D2-B14.md", "F13061", "F13080", 5320),
    ("UNX-D2-B15.md", "F13081", "F13100", 6000),
    ("UNX-D2-B16.md", "F13101", "F13120", 5800),
    ("UNX-D2-B17.md", "F13121", "F13140", 5700),
    ("UNX-D2-B18.md", "F13141", "F13160", 5600),
    ("UNX-D2-B19.md", "F13161", "F13180", 5500),
    ("UNX-D2-B20.md", "F13181", "F13200", 5400),
    ("UNX-D2-B21.md", "F13201", "F13220", 5300),
    ("UNX-D2-B22.md", "F13221", "F13240", 5400),
    ("UNX-D2-B23.md", "F13241", "F13260", 5500),
    ("UNX-D2-B24.md", "F13261", "F13280", 5600),
    ("UNX-D2-B25.md", "F13281", "F13300", 5700),
    ("UNX-D2-B26.md", "F13301", "F13320", 5500),
    ("UNX-D2-B27.md", "F13321", "F13340", 5400),
    ("UNX-D2-B28.md", "F13341", "F13360", 5600),
    ("UNX-D2-B29.md", "F13361", "F13380", 5800),
    ("UNX-D2-B30.md", "F13381", "F13400", 5800),
    ("UNX-D2-B31.md", "F13401", "F13420", 7230),
    ("UNX-D2-B32.md", "F13421", "F13440", 7230),
    ("UNX-D2-B33.md", "F13441", "F13460", 7230),
    ("UNX-D2-B34.md", "F13461", "F13480", 7230),
    ("UNX-D2-B35.md", "F13481", "F13500", 7230),
    ("UNX-D2-B36.md", "F13501", "F13520", 7230),
    ("UNX-D2-B37.md", "F13521", "F13540", 7230),
    ("UNX-D2-B38.md", "F13541", "F13560", 7230),
    ("UNX-D2-B39.md", "F13561", "F13580", 7230),
    ("UNX-D2-B40.md", "F13581", "F13600", 7230),
]

fails = []
ids_seen = []
total = 0
for fname, first, last, claimed in BATCHES:
    with open(f"{BASE}/{fname}", encoding="utf-8") as f:
        text = f.read()
    entries = re.findall(r"### (UNX-F(\d+)) · (.+)", text)
    rows = re.findall(r"纯功能行数：(\d+)", text)
    crits = re.findall(r"判据：UNX-(F\d+)-J1 ", text)
    states = re.findall(r"状态：\[([^\]]+)\]", text)
    # 1. 每批 20 条
    if len(entries) != 20:
        fails.append(f"{fname}: 条数 {len(entries)} != 20")
    if len(rows) != 20:
        fails.append(f"{fname}: 行数声明 {len(rows)} != 20")
    if len(crits) != 20:
        fails.append(f"{fname}: 判据 {len(crits)} != 20")
    if len(states) != 20:
        fails.append(f"{fname}: 状态 {len(states)} != 20")
    elif any(s != "骨架" for s in states):
        fails.append(f"{fname}: 存在非[骨架]状态 {set(states)}")
    # 2. ID 连续且落在批区间
    nums = [int(e[1]) for e in entries]
    if nums != list(range(int(first[1:]), int(last[1:]) + 1)):
        fails.append(f"{fname}: ID 序列不连续或不符区间 {first}-{last}")
    for e, c in zip(entries, crits):
        if e[0] != f"UNX-{c}":
            fails.append(f"{fname}: 判据编号与条目不配对 {e[0]} vs {c}")
    ids_seen.extend(e[0] for e in entries)
    # 3. 行数求和
    s = sum(int(r) for r in rows)
    if s != claimed:
        fails.append(f"{fname}: 行数求和 {s} != 批头登记 {claimed}")
    # 4. 行数区间 120-600
    for n, r in zip(nums, rows):
        if not (120 <= int(r) <= 600):
            fails.append(f"{fname}: F{n} 行数 {r} 越界 120-600")
    total += s
    print(f"{fname}: {len(entries)} 条 | 求和 {s} | 登记 {claimed} | {'OK' if s==claimed else 'FAIL'}")

# 5. 全域 ID 唯一 + 无空洞（B01-B40 满账段）
all_ints = [int(i.split("F")[1]) for i in ids_seen]
if len(set(ids_seen)) != 800:
    fails.append("全域 ID 存在重复")
if all_ints != list(range(12801, 13601)):
    fails.append("全域 ID 段 F12801-F13600 有空洞或乱序")
# 6. 域累计守恒（满账）
if total != 240000:
    fails.append(f"域累计 {total} != 240,000")

print(f"\n域累计（B01-B40 骨架锁定）: {total:,} / 240,000 {'—— 域满账' if total==240000 else ''}（40 批 / 800 条）")
if fails:
    print("\n== 断言失败 ==")
    for x in fails: print(" -", x)
    sys.exit(1)
print("== 全部断言通过 ==")
