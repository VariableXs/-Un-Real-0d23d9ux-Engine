# -*- coding: utf-8 -*-
"""UNX-D2 B01-B30 深化册断言链机械校验器（AI-17 会话 · finalize 五步 ①③⑤ 步）
校验：30 册 × 20 条；每条六要素（判据行/定位/语义边界/依赖与嫁接源/风险与回退/正文）齐备；
状态全部 [已深化]；每条字数 >=300；行数分解之和 == 纯功能行数；批求和 == 批头登记；
域累计 == 167,700；全域 ID F12801-F13400 无重复无空洞。
"""
import re, sys, io
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")

DEEPEN = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/docs/unxreal/deepen"
BATCHES = [
    ("D2-B01.md", "F12801", "F12820", 5800),
    ("D2-B02.md", "F12821", "F12840", 5920),
    ("D2-B03.md", "F12841", "F12860", 5540),
    ("D2-B04.md", "F12861", "F12880", 5520),
    ("D2-B05.md", "F12881", "F12900", 5680),
    ("D2-B06.md", "F12901", "F12920", 5300),
    ("D2-B07.md", "F12921", "F12940", 5360),
    ("D2-B08.md", "F12941", "F12960", 5400),
    ("D2-B09.md", "F12961", "F12980", 5580),
    ("D2-B10.md", "F12981", "F13000", 5320),
    ("D2-B11.md", "F13001", "F13020", 6360),
    ("D2-B12.md", "F13021", "F13040", 5300),
    ("D2-B13.md", "F13041", "F13060", 5700),
    ("D2-B14.md", "F13061", "F13080", 5320),
    ("D2-B15.md", "F13081", "F13100", 6000),
    ("D2-B16.md", "F13101", "F13120", 5800),
    ("D2-B17.md", "F13121", "F13140", 5700),
    ("D2-B18.md", "F13141", "F13160", 5600),
    ("D2-B19.md", "F13161", "F13180", 5500),
    ("D2-B20.md", "F13181", "F13200", 5400),
    ("D2-B21.md", "F13201", "F13220", 5300),
    ("D2-B22.md", "F13221", "F13240", 5400),
    ("D2-B23.md", "F13241", "F13260", 5500),
    ("D2-B24.md", "F13261", "F13280", 5600),
    ("D2-B25.md", "F13281", "F13300", 5700),
    ("D2-B26.md", "F13301", "F13320", 5500),
    ("D2-B27.md", "F13321", "F13340", 5400),
    ("D2-B28.md", "F13341", "F13360", 5600),
    ("D2-B29.md", "F13361", "F13380", 5800),
    ("D2-B30.md", "F13381", "F13400", 5800),
]

fails = []
ids_seen = []
total = 0
entry_total = 0
char_short = []

for fname, first, last, claimed in BATCHES:
    path = f"{DEEPEN}/{fname}"
    try:
        with open(path, encoding="utf-8") as f:
            text = f.read()
    except FileNotFoundError:
        fails.append(f"{fname}: 文件不存在")
        continue
    # 按 ### UNX-Fxxxxx 分条
    chunks = re.split(r"(?=### UNX-F\d+ · )", text)
    chunks = [c for c in chunks if c.startswith("### UNX-F")]
    entries = re.findall(r"### (UNX-F(\d+)) · ", text)
    crits = re.findall(r"判据：UNX-(F\d+)-J1 ", text)
    states = re.findall(r"状态：\[([^\]]+)\]", text)
    rows = re.findall(r"纯功能行数：(\d+) 行", text)
    decomp = re.findall(r"纯功能行数：(\d+) 行（(.+?);；?测试段不计）", text, re.S)

    if len(entries) != 20:
        fails.append(f"{fname}: 条目数 {len(entries)} != 20")
    if len(crits) != 20:
        fails.append(f"{fname}: 判据行数 {len(crits)} != 20")
    if len(states) != 20:
        fails.append(f"{fname}: 状态数 {len(states)} != 20")
    elif any(s != "已深化" for s in states):
        fails.append(f"{fname}: 存在非[已深化]状态 {set(states)}")
    # ID 连续
    nums = [int(e[1]) for e in entries]
    if nums != list(range(int(first[1:]), int(last[1:]) + 1)):
        fails.append(f"{fname}: ID 序列不连续或不符区间 {first}-{last}")
    for e, c in zip(entries, crits):
        if e[0] != f"UNX-{c}":
            fails.append(f"{fname}: 判据编号与条目不配对 {e[0]} vs UNX-{c}")
    ids_seen.extend(e[0] for e in entries)

    # 行数分解求和 == 声明
    if len(rows) != 20:
        fails.append(f"{fname}: 行数声明 {len(rows)} != 20")
    s = sum(int(r) for r in rows)
    if s != claimed:
        fails.append(f"{fname}: 行数求和 {s} != 批头登记 {claimed}")

    # 每条六要素 + 字数 + 分解守恒
    for i, ch in enumerate(chunks):
        m = re.match(r"### (UNX-F\d+) · ", ch)
        eid = m.group(1)
        need = ["**定位**", "**语义边界**", "**依赖与嫁接源**", "**风险与回退**", "状态：[已深化]"]
        for kw in need:
            if kw not in ch:
                fails.append(f"{fname}: {eid} 缺要素 {kw}")
        # 正文三步实现
        if "第一步" not in ch or "第二步" not in ch or "第三步" not in ch:
            fails.append(f"{fname}: {eid} 正文缺三步实现路径")
        # 衔接点/Windows 对照/复测方式
        for kw in ["与现存内核衔接点", "与 Windows 对照", "复测方式"]:
            if kw not in ch:
                fails.append(f"{fname}: {eid} 缺 {kw}")
        # 字数（判据行起点到条目结束，去空白）
        body = re.sub(r"\s", "", ch)
        if len(body) < 300:
            char_short.append(f"{fname}:{eid}:{len(body)}")
        # 分解求和
        dm = re.search(r"纯功能行数：(\d+) 行（(.+?)[;；]测试段不计）", ch, re.S)
        if dm:
            declared = int(dm.group(1))
            parts = [int(p) for p in re.findall(r"(\d+)\s*(?:\+|$)", dm.group(2).split("；")[0])]
            if sum(parts) != declared:
                fails.append(f"{fname}: {eid} 行数分解和 {sum(parts)} != 声明 {declared}")
        else:
            fails.append(f"{fname}: {eid} 行数分解格式未匹配")
    entry_total += len(entries)
    total += s
    min_c = min((len(re.sub(r'\s', '', c)) for c in chunks if c.startswith('### UNX-F')), default=0)
    print(f"{fname}: {len(entries)} 条 | 求和 {s} | 登记 {claimed} | 最短条 {min_c} 字 | {'OK' if s==claimed else 'FAIL'}")

# 全域
all_ints = [int(i.split("F")[1]) for i in ids_seen]
if len(set(ids_seen)) != 600:
    fails.append("全域 ID 存在重复")
if all_ints != list(range(12801, 13401)):
    fails.append("全域 ID 段 F12801-F13400 有空洞或乱序")
if total != 167700:
    fails.append(f"域累计 {total} != 167,700")
if entry_total != 600:
    fails.append(f"全域条目 {entry_total} != 600")
if char_short:
    fails.append(f"字数不足 300 的条目 {len(char_short)} 条: {char_short[:10]}")

print(f"\n深化册总数: 30 | 条目: {entry_total} | 域累计: {total:,} / 240,000（深化不改域账）")
if fails:
    print("\n== 断言失败 ==")
    for x in fails:
        print(" -", x)
    sys.exit(1)
print("== 深化册全部断言通过（六要素齐备 / ≥300 字 / 行数守恒 / 状态 [已深化]）==")
