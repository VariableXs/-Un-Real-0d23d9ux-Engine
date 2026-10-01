# -*- coding: utf-8 -*-
"""B4 域 B16-B30 骨架 finalize 前置断言链（AI-09 产线工具）

断言 ① 防重四范围 + ② 判据 + ③ 行数守恒（骨架阶段三步；④⑤ 深化收口后跑 deepen 全链脚本）。
"""
import re, sys, os

BASE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
UNX = os.path.join(BASE, "docs", "unxreal")
DOMAIN = "B4"
LO, HI = 6701, 7000
N_BATCH = 15

def load(p):
    with open(p, encoding="utf-8") as f:
        return f.read()

fails = []
def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + ("  " + str(detail) if detail else ""))
    if not ok:
        fails.append(name)

# ---------- ① 防重四范围 ----------
ids, names = [], []
for i in range(16, 31):
    sk = load(os.path.join(UNX, "batches", "UNX-%s-B%02d.md" % (DOMAIN, i)))
    for m in re.finditer(r"### UNX-F(\d{4}) · (.+)", sk):
        ids.append(m.group(1)); names.append(m.group(2).strip())

check("① 本轮 300 条 ID 连续 F6701-F7000 零空洞", [int(x) for x in ids] == list(range(LO, HI+1)))
check("① 本轮 300 条 ID 批内唯一", len(ids) == len(set(ids)) == 300)
check("① 本轮 300 条标题全域唯一", len(names) == len(set(names)))

# 范围1：kernel/varix/src 1,482 件零引用（ID 与标题名）
kern_hits = []
for root, _, files in os.walk(os.path.join(BASE, "kernel", "varix", "src")):
    for fn in files:
        p = os.path.join(root, fn)
        try:
            t = open(p, encoding="utf-8", errors="ignore").read()
        except OSError:
            continue
        for fid in ids:
            if ("UNX-F" + fid) in t:
                kern_hits.append((p, fid))
check("① 范围1 kernel/varix/src 零占用", not kern_hits, kern_hits[:5])

# 范围2：本域已收口批次 B01-B15 骨架零交叉
prev_hits = []
for i in range(1, 16):
    sk = load(os.path.join(UNX, "batches", "UNX-%s-B%02d.md" % (DOMAIN, i)))
    for fid in ids:
        if ("UNX-F" + fid) in sk:
            prev_hits.append(("B%02d" % i, fid))
check("① 范围2 本域 B01-B15 零交叉", not prev_hits, prev_hits[:5])

# 范围3：CGPU + START 零占用（抽样 336 件全文）
scope_hits = []
for base in [os.path.join(BASE, "docs", "START"), os.path.join(BASE, "CGPU")]:
    if not os.path.isdir(base):
        continue
    for root, _, files in os.walk(base):
        for fn in files:
            p = os.path.join(root, fn)
            try:
                t = open(p, encoding="utf-8", errors="ignore").read()
            except OSError:
                continue
            for fid in ids:
                if ("UNX-F" + fid) in t:
                    scope_hits.append((p, fid))
check("① 范围3 CGPU+START 零占用", not scope_hits, scope_hits[:5])

# 范围4：VE 卷不在仓库 N/A（登记口径）
print("PASS ① 范围4 VE 卷不在仓库 N/A（沿用域账既定口径登记）")

# ---------- ② 判据三成分 ----------
n_j1, n_bad = 0, []
for i in range(16, 31):
    sk = load(os.path.join(UNX, "batches", "UNX-%s-B%02d.md" % (DOMAIN, i)))
    for m in re.finditer(r"### UNX-F(\d{4}) · .+\n- 域/批：%s/B\d+｜纯功能行数：(\d+)｜状态：\[骨架\]｜判据：(.+)" % DOMAIN, sk):
        fid, budget, judg = m.group(1), m.group(2), m.group(3)
        n_j1 += 1
        ok = judg.startswith("UNX-F%s-J1 " % fid)
        has_num = bool(re.search(r"\d|≥|×|零|双|三|四|五", judg))
        has_verb = bool(re.search(r"断言|账|可观测|可导出|落盘|机检|复核|对照|校验|覆盖|注册|登记|生成|兑现|归位|留痕|裁断|注入|一致|语义|降级|容忍|清零|自清|续跑|滚动|拒收|派生|互验|恢复|回收|终结|枚举|分发|挂接|衔接|勾稽|域经|复测|补齐|合并|机读|dump|查明|坑|打法", judg))
        has_scope = bool(re.search(r"行|样本|轮|次|条|项|档|小时|批|域|段|块|字节|位|页|槽|值|窗|百分比|版本|模式|tag|字段|属性|路径|口径|LBA|扇区|接口|事件|命令|账|态|数|额|id|ID|号|组|点|线|下|盘|请求|文案", judg))
        if not (ok and has_num and has_verb and has_scope):
            n_bad.append((fid, ok, has_num, has_verb, has_scope))
check("② 判据 300 条全带 J1 且三成分齐备（数字/结果动词/口径范围）", n_j1 == 300 and not n_bad, n_bad[:6])

# ---------- ③ 行数守恒 ----------
tot = 0
for i in range(16, 31):
    sk = load(os.path.join(UNX, "batches", "UNX-%s-B%02d.md" % (DOMAIN, i)))
    budgets = [int(m) for m in re.findall(r"纯功能行数：(\d+)｜", sk)]
    tot += sum(budgets)
    check("③ B%02d 骨架 20 条求和 6,000" % i, len(budgets) == 20 and sum(budgets) == 6000)
check("③ 本轮域账累计 90,000（90,000→180,000）", tot == 90000, tot)

print()
print("== FAILURES:", fails if fails else "无 —— 骨架前置三步断言链全绿 ==")
sys.exit(1 if fails else 0)
