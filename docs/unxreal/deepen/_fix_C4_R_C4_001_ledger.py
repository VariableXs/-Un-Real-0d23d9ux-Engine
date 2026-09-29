# -*- coding: utf-8 -*-
"""C4 域账实修正（R-C4-001）：B02 骨架批头虚记 5,500 vs 逐条求和真值 5,460，差 40。
以逐条求和为唯一真值（沿 R-A1-004 先例），修正全账链：
deepen 批头+finalize 表（B02–B15）、batches 骨架批头（B02–B15）、B15 册内、总纲（C4 表 15 行字数+域小结）。
"""
import re
import sys

DEEPEN = "docs/unxreal/deepen"
BATCHES = "docs/unxreal/batches"
ZG = "docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal · 总纲与施工书.md"

# 域累计修正映射：批号 -> (旧, 新)
DOM = {
    "B02": ("11,340", "11,300"),
    "B03": ("17,020", "16,980"),
    "B04": ("22,800", "22,760"),
    "B05": ("28,640", "28,600"),
    "B06": ("34,240", "34,200"),
    "B07": ("39,740", "39,700"),
    "B08": ("45,420", "45,380"),
    "B09": ("51,040", "51,000"),
    "B10": ("56,440", "56,400"),
    "B11": ("62,400", "62,360"),
    "B12": ("68,180", "68,140"),
    "B13": ("73,740", "73,700"),
    "B14": ("79,500", "79,460"),
    "B15": ("85,720", "85,680"),
}
# 批内求和（仅 B02 变）
PIS = {nn: {"B02": ("5,500", "5,460")}.get(nn) for nn in DOM}

# 新 wc -m 字数（总纲 15 行回填）
WORDS = {
    "B01": "21,782", "B02": "19,012", "B03": "19,643", "B04": "18,679",
    "B05": "18,699", "B06": "18,999", "B07": "17,792", "B08": "18,071",
    "B09": "18,596", "B10": "18,044", "B11": "18,052", "B12": "17,179",
    "B13": "16,427", "B14": "16,458", "B15": "19,499",
}

def apply(fn, fixes):
    """fixes: list of (old, new, expected_count, tag)"""
    with open(fn, encoding="utf-8") as f:
        text = f.read()
    fail = []
    for old, new, cnt, tag in fixes:
        c = text.count(old)
        if c != cnt:
            fail.append(f"{fn} [{tag}] 旧串命中 {c} 次（预期 {cnt}）: {old[:60]}...")
            continue
        text = text.replace(old, new)
    if fail:
        print("\n".join(fail))
        return False
    with open(fn, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    print(f"OK {fn} ({len(fixes)} fixes)")
    return True

ok = True

# --- A. deepen 批头 + finalize 表 ---
for nn, (od, nd) in DOM.items():
    fn = f"{DEEPEN}/C4-{nn}.md"
    fixes = []
    # 批头
    fixes.append((f"域累计 {od}/240,000", f"域累计 {nd}/240,000", 1, "批头域累计"))
    # finalize 表
    if nn == "B02":
        fixes.append((
            "| 批内行数求和 | 5,500 行（20 条逐条求和，与批次账一致） |",
            "| 批内行数求和 | 5,460 行（R-C4-001 修正后真值：20 条逐条求和，原骨架批头虚记 5,500 已改） |",
            1, "B02 finalize 求和行"))
        fixes.append((
            "| 域累计 | 11,340 / 240,000（账余 228,660） |",
            "| 域累计 | 11,300 / 240,000（账余 228,700） |",
            1, "B02 finalize 域累计行"))
        fixes.append(("③行数守恒：5,500 = 批次账", "③行数守恒：5,460 = 批次账", 1, "B02 finalize 断言行"))
    elif nn in ("B03", "B04"):
        old_dom = od
        old_rest = {"B03": "222,980", "B04": "217,200"}[nn]
        new_rest = {"B03": "223,020", "B04": "217,240"}[nn]
        fixes.append((
            f"| 域累计 | {old_dom} / 240,000（账余 {old_rest}） |",
            f"| 域累计 | {nd} / 240,000（账余 {new_rest}） |",
            1, f"{nn} finalize 域累计行"))
    else:
        fixes.append((
            f"| UNX-C4-{nn} | 20 | ", f"| UNX-C4-{nn} | 20 | ", 0, "skip"))  # placeholder removed below
        fixes.pop()
        # 四列表：| UNX-C4-BNN | 20 | X | OLD / 240,000 |
        # 批内求和不变，仅域累计变
        fixes.append((
            f"/ 240,000 |\n\n- finalize 断言链", f"/ 240,000 |\n\n- finalize 断言链", 0, "skip2"))
        fixes.pop()
        fixes.append((f"| {od} / 240,000 |", f"| {nd} / 240,000 |", 1, f"{nn} finalize 四列表"))
    if nn == "B15":
        fixes.append(("域账收口（85,720/240,000 · 300 条 finalize 全过）", "域账收口（85,680/240,000 · 300 条 finalize 全过）", 1, "B15 判据行"))
        fixes.append(("域账（85,720/240,000 行、300 条）", "域账（85,680/240,000 行、300 条）", 1, "B15 定位行"))
        fixes.append(("逐批核=85,720", "逐批核=85,680", 1, "B15 正文行"))
    ok = apply(fn, fixes) and ok

# --- B. batches 骨架批头 ---
SKELETON_CHAIN = {
    "B02": ("B01 5,840 + 本批 5,500 = 11,340", "B01 5,840 + 本批 5,460 = 11,300"),
    "B03": ("B01–B02 11,340 + 本批 5,680 = 17,020", "B01–B02 11,300 + 本批 5,680 = 16,980"),
    "B04": ("B01–B03 17,020 + 本批 5,780 = 22,800", "B01–B03 16,980 + 本批 5,780 = 22,760"),
    "B05": ("B01–B04 22,800 + 本批 5,840 = 28,640", "B01–B04 22,760 + 本批 5,840 = 28,600"),
    "B06": ("B01–B05 28,640 + 本批 5,600 = 34,240", "B01–B05 28,600 + 本批 5,600 = 34,200"),
    "B07": ("B01–B06 34,240 + 本批 5,500 = 39,740", "B01–B06 34,200 + 本批 5,500 = 39,700"),
    "B08": ("B01–B07 39,740 + 本批 5,680 = 45,420", "B01–B07 39,700 + 本批 5,680 = 45,380"),
    "B09": ("B01–B08 45,420 + 本批 5,620 = 51,040", "B01–B08 45,380 + 本批 5,620 = 51,000"),
    "B10": ("B01–B09 51,040 + 本批 5,400 = 56,440", "B01–B09 51,000 + 本批 5,400 = 56,400"),
    "B11": ("B01–B10 56,440 + 本批 5,960 = 62,400", "B01–B10 56,400 + 本批 5,960 = 62,360"),
    "B12": ("B01–B11 62,400 + 本批 5,780 = 68,180", "B01–B11 62,360 + 本批 5,780 = 68,140"),
    "B13": ("B01–B12 68,180 + 本批 5,560 = 73,740", "B01–B12 68,140 + 本批 5,560 = 73,700"),
    "B14": ("B01–B13 73,740 + 本批 5,760 = 79,500", "B01–B13 73,700 + 本批 5,760 = 79,460"),
    "B15": ("B01–B14 79,500 + 本批 6,220 = 85,720", "B01–B14 79,460 + 本批 6,220 = 85,680"),
}
for nn, (old, new) in SKELETON_CHAIN.items():
    ok = apply(f"{BATCHES}/UNX-C4-{nn}.md", [(old, new, 1, "骨架批头")]) and ok

# --- C. 总纲 ---
with open(ZG, encoding="utf-8") as f:
    zg = f.read()

# C1: C4 表 15 行字数更新 + B02 行批注改写
lines = zg.split("\n")
zg_fixes = 0
for i, ln in enumerate(lines):
    m = re.match(r"\| UNX-C4-(B\d\d) \|", ln)
    if not m or "已深化" not in ln:
        continue
    nn = m.group(1)
    new_line = re.sub(r"正文 [\d,]+ 字（wc -m 实计）", f"正文 {WORDS[nn]} 字（wc -m 实计）", ln)
    if nn == "B02":
        new_line = new_line.replace(
            "· 5,500 行锁定零偏离（PIPE_BUF 原子性/FIFO 节点/splice 预留）",
            "· 5,460 行锁定零偏离（R-C4-001 账实修正：原骨架批头虚记 5,500，以逐条求和为唯一真值）（PIPE_BUF 原子性/FIFO 节点/splice 预留）",
        )
    if new_line != ln:
        lines[i] = new_line
        zg_fixes += 1
zg = "\n".join(lines)
print(f"总纲 C4 表行更新: {zg_fixes} 行")
if zg_fixes != 15:
    print(f"FAIL: 预期 15 行，实际 {zg_fixes}")
    ok = False

# C2: 域小结
for old, new, tag in [
    ("批内求和合计 85,720/240,000 行，三态/字数/行数已回填上表",
     "批内求和合计 85,680/240,000 行（R-C4-001 账实修正：B02 骨架批头虚记 5,500 vs 逐条真值 5,460，差 40 已改），三态/字数/行数已回填上表",
     "域小结求和"),
    ("账余 154,280 行", "账余 154,320 行", "域小结账余"),
]:
    c = zg.count(old)
    if c != 1:
        print(f"FAIL 域小结 [{tag}] 命中 {c}")
        ok = False
    else:
        zg = zg.replace(old, new)
        print(f"OK 总纲域小结 [{tag}]")

with open(ZG, "w", encoding="utf-8", newline="\n") as f:
    f.write(zg)

print("ALL OK" if ok else "HAS FAILURES")
sys.exit(0 if ok else 1)
