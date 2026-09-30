# -*- coding: utf-8 -*-
# A1 域 R-A1-004 账实修正：B02 批小计 6,000（骨架估算虚记）→ 5,980（逐条求和真值）
# 差额 20 行由 B40 批内配平（F0800 580→600，批小计 11,500→11,520），全链累计声明 -20，终点保持 240,000
# 用法: python _fix_B02_chain.py
import os, re, io

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
BATCH = os.path.join(ROOT, "docs", "unxreal", "batches")
BOOK = os.path.join(ROOT, "docs", "unxreal", "deepen")
ZG = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal",
                  "CoRun Varix STAR II · Unxreal · 总纲与施工书.md")

def rd(p): return io.open(p, encoding="utf-8").read()
def wr(p, t): io.open(p, "w", encoding="utf-8", newline="\n").write(t)
def fmt(v, comma): return "{:,}".format(v) if comma else str(v)
def sub1(txt, old, new, tag):
    c = txt.count(old)
    if c == 0 and txt.count(new) >= 1:
        return txt  # 幂等：已替换过
    assert c == 1, (tag, c, old[:60])
    return txt.replace(old, new)

# ---------- 1. B02 册头账实修正 ----------
p = os.path.join(BOOK, "A1-B02.md")
t = rd(p)
t = sub1(t,
    "批累计行数锁定 6,000（= 样板 380+340 + 本册 18 条之和）",
    "批累计行数锁定 5,980（= 样板 380+340 + 本册 18 条之和 5,260；R-A1-004 账实修正：原记 6,000 为骨架估算虚记，逐条求和 5,980 为唯一真值，finalize 会话修正并全域复验通过）",
    "B02-head")
wr(p, t); print("B02 book head fixed")

# ---------- 2. 总纲：账表行 + 骨架叙述注记 ----------
t = rd(ZG)
t = sub1(t,
    "| UNX-A1-B02 | F0021–F0040 | 20 | [已深化] | 正文 9,799 字（wc -m 实计；样板 F0021/F0039 不计新深化）· 6,000 行锁定零偏离 |",
    "| UNX-A1-B02 | F0021–F0040 | 20 | [已深化] | 正文 9,799 字（wc -m 实计；样板 F0021/F0039 不计新深化）· 5,980 行锁定零偏离（R-A1-004 账实修正：原记 6,000 为骨架估算虚记，逐条求和 5,980 为唯一真值，finalize 会话修正） |",
    "ZG-table")
t = sub1(t,
    "B02 小计 6,000 行，域 A1 账余 228,380 行——账面上明确\"骨架期行数为估算值\"",
    "B02 小计 6,000 行（骨架估算；实锁 5,980，R-A1-004 账实修正见批次账表），域 A1 账余 228,380 行——账面上明确\"骨架期行数为估算值\"",
    "ZG-1349")
t = sub1(t,
    "B01 小计 5,620 + B02 小计 6,000 = 11,620 行，域 A1 账余 228,380（240,000 − 11,620）",
    "B01 小计 5,620 + B02 小计 5,980（R-A1-004 账实修正后真值；骨架估算原记 6,000）= 11,600 行，域 A1 账余 228,400（240,000 − 11,600）",
    "ZG-1608")
wr(ZG, t); print("总纲 fixed")

# ---------- 3. B03 特记批册头部 + 册累计 ----------
p = os.path.join(BATCH, "UNX-A1-B03.md")
t = rd(p)
t = sub1(t, "域账累计：B01 5,620 + B02 6,000 + 本批 5,500 = 17,120 / 240,000",
            "域账累计：B01 5,620 + B02 5,980（R-A1-004 账实修正）+ 本批 5,500 = 17,100 / 240,000", "B03-batch")
wr(p, t)
p = os.path.join(BOOK, "A1-B03.md")
t = rd(p)
c = t.count("域累计 17,120")
if c == 0 and "域累计 17,100" in t:
    print("B03 fixed (already)")
else:
    assert c == 2, c  # 册头 + finalize 表锁行两处同一累计声明
    t = t.replace("域累计 17,120", "域累计 17,100")
    wr(p, t); print("B03 fixed")

# ---------- 4. B04–B40 批册头部链修正 ----------
for i in range(4, 41):
    bn = "B%02d" % i
    p = os.path.join(BATCH, "UNX-A1-%s.md" % bn)
    t = rd(p)
    m = re.search(r"域账累计：([\d,]+) \+ 本批 ([\d,]+) = ([\d,]+) / 240,000", t)
    assert m, bn
    prev, tot, cum = (int(x.replace(",", "")) for x in m.groups())
    cm = "," in m.group(1)
    if bn == "B40":
        nprev, ntot, ncum = 228480, 11520, 240000
    else:
        nprev, ntot, ncum = prev - 20, tot, cum - 20
    old = m.group(0)
    new = "域账累计：%s + 本批 %s = %s / 240,000" % (fmt(nprev, cm), fmt(ntot, cm), fmt(ncum, cm))
    t = sub1(t, old, new, bn)
    # B40 留痕行批小计同步
    if bn == "B40":
        t = sub1(t, "本批逐条求和 11500", "本批逐条求和 11520", "B40-note")
        t = sub1(t, "本批头部批小计 11500 为逐条求和后落账", "本批头部批小计 11520 为逐条求和后落账", "B40-note2")
        t = sub1(t, "｜纯功能行数：580｜状态：[已深化]｜判据：UNX-F0800-J1",
                    "｜纯功能行数：600｜状态：[已深化]｜判据：UNX-F0800-J1", "B40-f0800")
    wr(p, t)
    print(bn, "batch head:", old[:40], "->", new[:40])

# ---------- 5. B19–B40 深化册锁行链修正 ----------
for i in range(19, 41):
    bn = "B%02d" % i
    p = os.path.join(BOOK, "A1-%s.md" % bn)
    t = rd(p)
    m = re.search(r"域账累计 ([\d,]+) / 240,000", t)
    assert m, bn
    v = int(m.group(1).replace(",", ""))
    cm = "," in m.group(1)
    nv = 240000 if bn == "B40" else v - 20
    old = m.group(0)
    new = "域账累计 %s / 240,000" % fmt(nv, cm)
    if old != new:
        c = t.count(old)
        assert 1 <= c <= 2, (bn + "-lock", c)  # 收口册（起步+续批两段）最多 2 处锁行
        t = t.replace(old, new)
    # B40 批小计全套同步
    if bn == "B40":
        t = sub1(t, "域账本批锁定 11500 行", "域账本批锁定 11520 行", "B40-head")
        t = sub1(t, "｜纯功能行数：580 行（宣告执行 200 + 终局核 190 + 账目 190；测试段不计）",
                    "｜纯功能行数：600 行（宣告执行 200 + 终局核 200 + 账目 200；测试段不计）", "B40-f0800")
        t = sub1(t, "域账累计 228,500 + 11,500 = 240,000 精确等", "域账累计 228,480 + 11,520 = 240,000 精确等", "B40-f0782loc")
        t = sub1(t, "（本批 11,500 一致、域终点 240,000）", "（本批 11,520 一致、域终点 240,000）", "B40-f0799loc")
        t = sub1(t, "全批逐条求和 11500 与批小计一致", "全批逐条求和 11520 与批小计一致", "B40-table")
    wr(p, t)
    print(bn, "book lock:", old, "->", new)

# ---------- 6. B40 数据模块同步 ----------
p = os.path.join(BOOK, "_data_A1_B40.py")
t = rd(p)
t = sub1(t, 'add("F0800","A1 域满收官终局宣告（800 条终态定格+全域闭域）",580,\n"UNX-F0800-J1 终局宣告过（全域 800 条终态定格 + 六判据凭证齐 6/6 + 封域交付签收三账齐）且域账终点 240,000 终版",\n"宣告执行 200 + 终局核 190 + 账目 190",',
            'add("F0800","A1 域满收官终局宣告（800 条终态定格+全域闭域）",600,\n"UNX-F0800-J1 终局宣告过（全域 800 条终态定格 + 六判据凭证齐 6/6 + 封域交付签收三账齐）且域账终点 240,000 终版",\n"宣告执行 200 + 终局核 200 + 账目 200",', "data-f0800")
t = sub1(t, "①终点宣告（B40 批小计 11,500 落账 → 域账累计 228,500 + 11,500 = 240,000 精确等——宣告账行）",
            "①终点宣告（B40 批小计 11,520 落账 → 域账累计 228,480 + 11,520 = 240,000 精确等——宣告账行）", "data-f0782")
t = sub1(t, "（本批 11,500 一致、域终点 240,000）", "（本批 11,520 一致、域终点 240,000）", "data-f0799")
wr(p, t); print("B40 data module synced")

print("\nFIX DONE: B02=5,980 真值修正 + 全链 -20 + B40 配平 11,520 → 终点 240,000")
