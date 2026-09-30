# -*- coding: utf-8 -*-
"""A3 域 finalize 五步断言链机械化校验（AI-03 产线工具 · v4）

v4 口径（二轮收官扩账）：B01–B30 全域前 30 批（40 批中的已领段）。
- ID 域 F1601–F2200（600 条）；骨架逐批求和 6,000；域账累计 180,000 / 240,000。
- 记账表双格式容忍：B01（首批老体例）无表可；B02–B30 必须恰有一张
  `## finalize 记账表`，且逐批复核行数合计加法链（20 项求和 = 6,000）。
- ④ 骨架册为判据/行数登记权威；深化册判据为同义转述（B01–B14 既定体例），
  故查 J1 号前缀一致而非逐字相等。
- 行数分解段以 "+" 分段、段内取末位数字为段额（容忍段名中的 72h/10 万等数字）。
"""
import re, sys, os

BASE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "docs", "unxreal")
N_BATCH = 30
DOMAIN = "A3"

def load(p):
    with open(p, encoding="utf-8") as f:
        return f.read()

fails = []
def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + ("  " + str(detail) if detail else ""))
    if not ok:
        fails.append(name)

# ---------- ① 防重：条目名/ID 唯一性（batches+deepen 两册全域） ----------
all_sk, all_dp = {}, {}
for i in range(1, N_BATCH + 1):
    sk = load(os.path.join(BASE, "batches", "UNX-%s-B%02d.md" % (DOMAIN, i)))
    dp = load(os.path.join(BASE, "deepen", "%s-B%02d.md" % (DOMAIN, i)))
    for m in re.finditer(r"### UNX-(F\d{4})", sk):
        all_sk.setdefault(m.group(1), []).append("B%02d" % i)
    for m in re.finditer(r"#{2,3} UNX-(F\d{4})", dp):
        all_dp.setdefault(m.group(1), []).append("B%02d" % i)
ids_expect = ["F%04d" % n for n in range(1601, 1601 + N_BATCH * 20)]
sk_ids, dp_ids = sorted(all_sk), sorted(all_dp)
check("① 骨架 ID 全域唯一且=%d 条" % (N_BATCH * 20),
      len(sk_ids) == N_BATCH * 20 and all(len(v) == 1 for v in all_sk.values()))
check("① 深化 ID 全域唯一且=%d 条" % (N_BATCH * 20),
      len(dp_ids) == N_BATCH * 20 and all(len(v) == 1 for v in all_dp.values()))
check("① 骨架/深化 ID 集一致且=F1601–F%d" % (1600 + N_BATCH * 20),
      sk_ids == dp_ids == ids_expect)

# ---------- ② 判据三成分 ----------
n_j1 = 0
for i in range(1, N_BATCH + 1):
    sk = load(os.path.join(BASE, "batches", "UNX-%s-B%02d.md" % (DOMAIN, i)))
    n_j1 += len(re.findall(r"判据：UNX-F\d{4}-J1 ", sk))
check("② 判据 %d 条全带 UNX-F####-J1" % (N_BATCH * 20), n_j1 == N_BATCH * 20, n_j1)

# ---------- ③ 行数守恒：骨架逐批求和=6,000，域累计 180,000 ----------
tot = 0
for i in range(1, N_BATCH + 1):
    sk = load(os.path.join(BASE, "batches", "UNX-%s-B%02d.md" % (DOMAIN, i)))
    budgets = [int(m) for m in re.findall(r"纯功能行数：(\d+)｜", sk)]
    tot += sum(budgets)
    check("③ B%02d 骨架 20 条求和 6,000" % i, len(budgets) == 20 and sum(budgets) == 6000)
check("③ 域账累计 %s" % "{:,}".format(N_BATCH * 6000), tot == N_BATCH * 6000, tot)

# ---------- ④ 深化册逐条：六要素 + 行数分解 + J1 前缀 + 正文≥300 字 ----------
def parse_deepen(dp):
    parts = re.split(r"\n(?=#{2,3} UNX-F\d{4} )", dp)
    out = {}
    for chunk in parts:
        m = re.match(r"#{2,3} UNX-(F\d{4}) ", chunk)
        if not m:
            continue
        fid = m.group(1)
        j = re.search(r"- \*\*判据\*\*：(.+)", chunk) or re.search(r"｜判据：(UNX-F\d{4}-J1 .+?)｜", chunk)
        ln = re.search(r"- \*\*行数\*\*：(\d+) 行（(.+)）。", chunk) or \
             re.search(r"纯功能行数：(\d+) 行（(.+?)）", chunk)
        body = re.search(r"- \*\*正文\*\*：(.+)", chunk) or re.search(r"\n- 正文：(.+)", chunk)
        out[fid] = {"judg": j.group(1) if j else None,
                    "total": int(ln.group(1)) if ln else None,
                    "parts": ln.group(2) if ln else None,
                    "body": body.group(1) if body else None,
                    "loc": bool(re.search(r"- \*\*定位\*\*：", chunk)),
                    "bound": bool(re.search(r"- \*\*语义边界\*\*：", chunk)),
                    "dep": bool(re.search(r"- \*\*依赖与嫁接源\*\*：", chunk)),
                    "risk": bool(re.search(r"- \*\*风险与回退\*\*：", chunk))}
    return out

def parts_sum(decomp):
    s = 0
    for seg in decomp.split("+"):
        nums = re.findall(r"\d+", seg)
        if nums:
            s += int(nums[-1])
    return s

for i in range(1, N_BATCH + 1):
    tag = "B%02d" % i
    sk = load(os.path.join(BASE, "batches", "UNX-%s-B%02d.md" % (DOMAIN, i)))
    dp = load(os.path.join(BASE, "deepen", "%s-B%02d.md" % (DOMAIN, i)))
    sk_e = re.findall(r"### UNX-(F\d{4}) · .+\n- 域/批：A3/B\d+｜纯功能行数：(\d+)｜状态：\[已深化\]｜判据：(.+)", sk)
    d = parse_deepen(dp)
    check("④ %s 深化 20 条全解析" % tag, len(d) == 20, len(d))
    if len(sk_e) != 20 or len(d) != 20:
        continue
    prob = []
    for fid, budget, judg in sk_e:
        e = d.get(fid)
        if not e:
            prob.append((fid, "missing")); continue
        if e["judg"] is None or e["total"] is None or e["body"] is None:
            prob.append((fid, "field-missing")); continue
        if not e["judg"].strip().startswith("UNX-%s-J1" % fid):
            prob.append((fid, "j1-prefix"))
        if e["total"] != int(budget):
            prob.append((fid, "total", e["total"], budget))
        elif parts_sum(e["parts"]) != e["total"]:
            prob.append((fid, "decomp", parts_sum(e["parts"]), e["total"]))
        if len(e["body"]) < 300:
            prob.append((fid, "body", len(e["body"])))
        if not (e["loc"] and e["bound"] and e["dep"] and e["risk"]):
            prob.append((fid, "six-elem"))
    check("④ %s 逐条五查（J1/总行数/分解/正文≥300/六要素）" % tag, not prob, prob if prob else "")

# ---------- ⑤ 记账表：双格式容忍（B01 老体例无表可），B02–B30 逐批加法链 ----------
for i in range(1, N_BATCH + 1):
    tag = "B%02d" % i
    dp = load(os.path.join(BASE, "deepen", "%s-B%02d.md" % (DOMAIN, i)))
    n_tbl = len(re.findall(r"^## finalize 记账表", dp, re.M))
    if i == 1:
        check("⑤ %s 记账表双格式容忍（0 或 1 张）" % tag, n_tbl in (0, 1), n_tbl)
        if n_tbl == 0:
            continue
    else:
        check("⑤ %s 记账表恰一张" % tag, n_tbl == 1, n_tbl)
    m = re.search(r"行数合计 \| 6,000（(.+?) = 6,000，零偏离）", dp)
    if m:
        nums = [int(x) for x in re.findall(r"\d+", m.group(1))]
        check("⑤ %s 记账表逐条加法=6,000（20 项）" % tag, sum(nums) == 6000 and len(nums) == 20,
              (sum(nums), len(nums)))
    else:
        check("⑤ %s 记账表行数声明" % tag, False, "未匹配")

print()
print("== FAILURES:", fails if fails else "无 —— 五步断言链机械面全绿 ==")
sys.exit(1 if fails else 0)
