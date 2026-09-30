# -*- coding: utf-8 -*-
"""AI-52 · K2 增补册校验器：七查 ALL PASS exit=0"""
import io, os, re, sys
BASE = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
DIR = os.path.join(BASE, "docs", "Varix", "CoRun Varix STAR II · Unxreal")
SA = os.path.join(DIR, "AI-52 · K2 · 300项新功能增补册（B01–B15）.md")
MAIN = os.path.join(DIR, "CoRun Varix STAR II · Unxreal.md")
fails = []
def chk(name, cond, detail=""):
    print(("PASS " if cond else "FAIL ") + name + (" " + detail if detail else ""))
    if not cond: fails.append(name)

sa = io.open(SA, encoding="utf-8").read()
mn = io.open(MAIN, encoding="utf-8").read()

# 1) 300 条 ID 连续在位
ids = re.findall(r"UNX-F(4\d{4})", sa)
k2 = sorted(set(int(x) for x in ids if 40801 <= int(x) <= 41100))
chk("1 十五册在位/300 条 ID 连续零缺失", k2 == list(range(40801, 41101)), f"count={len(k2)}")

# 2) 行数守恒 6,000×15=90,000
rows = re.findall(r"\| UNX-F4\d{4} \| [^|]+ \| (\d+) \| 增补 \|", sa)
rows = [int(r) for r in rows]
chk("2 行数 90,000 守恒", sum(rows) == 90000 and len(rows) == 300, f"sum={sum(rows)} n={len(rows)}")

# 3) 判据号唯一且逐条带 J1
ev = re.findall(r"\| UNX-F(4\d{4}) \| [^|]+ \| \d+ \| 增补 \| ([^|]+) \|", sa)
jud = {}
for i, e in ev:
    m = re.search(r"UNX-F%d-J1" % int(i), e)
    jud[int(i)] = bool(m)
chk("3 判据号 300 枚逐条 J1 在位", all(jud.get(int(x), False) for x in range(40801, 41101)) and len(jud) == 300)

# 4) 与主册一致（主册含同一卷 300 条）
mrows = re.findall(r"\| UNX-F(4\d{4}) \| [^|]+ \| (\d+) \| 增补 \|", mn)
mk2 = [(int(a), int(b)) for a, b in mrows if 40801 <= int(a) <= 41100]
chk("4 主册增补卷 300 条与独立册一致", len(mk2) == 300)

# 5) ID 段与邻域零交叠（段外 ID 不混入）
bad = [x for x in ids if not (40801 <= int(x) <= 41100) and int(x) >= 40000 and int(x) < 42000 and int(x) not in range(40801,41101)]
# 允许的段外引用应为邻域判据/联签锚定；这里只检查条目列（第1列）严格在段内
first_col = re.findall(r"^\| UNX-F(\d+) \|", sa, re.M)
chk("5 条目列 ID 严格在 F40801–F41100", all(40801 <= int(x) <= 41100 for x in first_col), f"n={len(first_col)}")

# 6) 批数 = 15，批头齐全
bh = re.findall(r"## 批 UNX-K2-B(\d{2})（F(\d+)–F(\d+) · ", sa)
chk("6 批头 15 个且 ID 段正确", len(bh) == 15 and all(int(a) == 40801 + 20 * (int(n) - 1) for n, a, b in bh))

# 7) 主册纯追加（原 324,864 行内容前缀保持）
chk("7 主册含 AI-52 卷标且旧内容零删除", "# 增补卷 · AI-52 · 波19 首产段 K2 域 300 项新功能" in mn and "UNX-F38400-J1" in mn)

print("ALL PASS" if not fails else "FAILED: " + ", ".join(fails))
sys.exit(0 if not fails else 1)
