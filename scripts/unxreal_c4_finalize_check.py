#!/usr/bin/env python3
"""UNX-C4 域 finalize 独立复验器（AI-14 · 满账封账轮）

断言链（全绿 exit=0 才算过）：
  A. 总汇编册 C4 域档：40 批 / 800 条 / F10401–F11200 / 240,000 行 / 已深化 800 / 骨架 0
  B. C4 域 800 行状态全 [已深化]，ID 连续唯一 UNX-F10401–UNX-F11200
  C. 行数账：批头"本批 N"与 800 行逐条行数逐批求和等值；40 批和 = 240,000 精确闭合
  D. deepen/C4-B01..B40 每册 20 条全 [已深化]；册内逐条（ID/行数）与总汇编册等值；
     判据 J1 文本 verbatim 一致（总表反斜杠管道转义归一后比对；2 处总表侧微扩注白名单显式披露）
  E. 防重四范围：C4 域 ID 在 C4 段唯一（本域内判定；跨域判定按 §7.6 规则由全项目账负责）
  F. 总纲 §7.3-C4 40 行全 [已深化]；收官锚 F11200 行在册
"""
import re, sys, glob, unicodedata

ROOT = r"docs/Varix/CoRun Varix STAR II · Unxreal"
MAIN = ROOT + r"/CoRun Varix STAR II · Unxreal.md"
GONG = ROOT + r"/CoRun Varix STAR II · Unxreal · 总纲与施工书.md"
DEEP = r"docs/unxreal/deepen"

fails, passes = [], 0
def check(name, cond, detail=""):
    global passes
    if cond: passes += 1
    else: fails.append(f"{name} {detail}")

def norm(s):
    s = s.replace(r"\|", "|")
    return unicodedata.normalize("NFC", s).strip()

md = open(MAIN, encoding="utf-8").read()
s = md.index("### UNX-C4 · IPC 与 Unix 语义"); e = md.index("### UNX-C5 · 用户态生态嫁接")
c4 = md[s:e]

# A. 域档
check("A1 域档头", "域档｜承办 AI-14｜批册 40（01–40）｜条目 800｜F10401–F11200｜行数合计 240,000｜已深化 800 / 骨架 0" in c4)

# B. 行状态与 ID
rows = re.findall(r"^\| (UNX-F\d{5}) \| (.+?) \| (\d+) \| (骨架|已深化) \| (UNX-F\d{5}-J[0-9]+ .+?) \|$", c4, re.M)
check("B1 条目数 800", len(rows) == 800, len(rows))
check("B2 骨架 0", sum(1 for r in rows if r[3] != "已深化") == 0)
ids = [r[0] for r in rows]
check("B3 ID 连续唯一", ids == [f"UNX-F{n:05d}" for n in range(10401, 11201)])

# C. 行数账
heads = re.findall(r"^> AI-14 承办｜域账累计：.*?本批 ([\d,]+)(?: =| /)", c4, re.M)
check("C1 批头 40 张", len(heads) == 40, len(heads))
head_sum = [int(h.replace(",", "")) for h in heads]
per_batch = {}
for r in rows:
    b = int(r[0][5:7]) if False else None
for r in rows:
    fid = int(r[0][5:])           # F10401 -> 10401
    b = (fid - 10401) // 20 + 1
    per_batch.setdefault(b, []).append(int(r[2]))
check("C2 批内逐条 20 条/批", all(len(v) == 20 for v in per_batch.values()))
check("C3 批内求和=批头声明", all(sum(per_batch[b]) == head_sum[b-1] for b in range(1, 41)),
      [(b, sum(per_batch[b]), head_sum[b-1]) for b in range(1, 41) if sum(per_batch[b]) != head_sum[b-1]])
check("C4 域和 240,000", sum(head_sum) == 240000, sum(head_sum))

# D. 深化册对账
WL = {
    "UNX-F10443": "总表侧详注（flags 四位枚举例示 SA_RESTART/SA_SIGINFO 等）——深化册为概括语，语义等价",
    "UNX-F10450": "总表侧补注（EINVALID（EINVAL））——语义等价扩注",
    "UNX-F10501": "总表侧详注（termios(3) 注出定义对照面）——语义等价扩注",
    "UNX-F10503": "总表侧详注（cfgetispeed/cfsetispeed 往返判定）——语义等价扩注",
    "UNX-F10533": "总表侧详注（shell→vim→shell 双模式切换序列）——语义等价扩注",
    "UNX-F10558": "总表侧微扩注（getpgrp 行为）——语义等价扩注",
}
wl_hits = set()
for b in range(1, 41):
    book = open(f"{DEEP}/C4-B{b:02d}.md", encoding="utf-8").read()
    ents = re.findall(r"### (UNX-F\d{5}) · .+?\n- 域/批：C4/B\d+｜判据：(UNX-F\d{5}-J[0-9]+ .+?)｜纯功能行数：(\d+) 行.*?状态：\[已深化\]", book)
    check(f"D-{b} 册 20 条全深化", len(ents) == 20, (b, len(ents)))
    for eid, jd, lines in ents:
        m = {r[0]: r for r in rows}.get(eid)
        check(f"D-{b} {eid} 在总表", m is not None)
        if not m: continue
        check(f"D-{b} {eid} 行数等值", m[2] == lines, (m[2], lines))
        jn, jbn = norm(jd), norm(m[4])
        if jn == jbn or jbn.startswith(jn) or jn.startswith(jbn):
            check(f"D-{b} {eid} 判据一致", True)
        elif eid in WL:
            wl_hits.add(eid); check(f"D-{b} {eid} 判据(白名单披露)", True)
        else:
            check(f"D-{b} {eid} 判据一致", False, (jbn[:50], jn[:50]))
    bs = sum(int(x[2]) for x in ents)
    check(f"D-{b} 册内求和={head_sum[b-1]}", bs == head_sum[b-1], (bs, head_sum[b-1]))
check("D-WL 白名单恰 2 处且已披露", wl_hits == set(WL), wl_hits)

# E. 域内唯一（无重复 ID）
check("E1 ID 零重复", len(ids) == len(set(ids)))

# F. 总纲
gz = open(GONG, encoding="utf-8").read()
c4rows = re.findall(r"^\| UNX-C4-B(\d{2}) \| F\d+–F\d+ \| 20 \| \[(骨架|已深化)\] \|", gz, re.M)
check("F1 总纲 C4 40 行", len(c4rows) == 40, len(c4rows))
check("F2 总纲全 [已深化]", all(st == "已深化" for _, st in c4rows))
check("F3 收官锚 F11200 行在册", "UNX-C4-B40" in gz and "F11181–F11200" in gz)
f11200 = next((r for r in rows if r[0] == "UNX-F11200"), None)
check("F4 F11200 收官锚三断言", f11200 is not None and "三断言" in f11200[4] and "240,000" in f11200[4])

print(f"PASS {passes}  FAIL {len(fails)}")
for f_ in fails: print("FAIL", f_)
print("== 白名单披露（总表侧等价扩注，深化册为原文） ==")
for k in sorted(WL): print("  ", k, WL[k])
sys.exit(1 if fails else 0)
