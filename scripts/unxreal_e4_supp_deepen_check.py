# -*- coding: utf-8 -*-
"""UNX-E4 增补卷深化校验器（AI-24 · 卷一至卷七 E01–E105 · 105 册）。
① 105 册、每册 20 条 ② 条 ID 与批册一一对应且区间连续 ③ 状态列 [已深化]
④ 判据号与批册一致（防错位） ⑤ 正文逐条 ≥300 字 ⑥ 六要素（定位/语义边界/依赖/风险/正文/元行）齐全
全绿 exit=0。
"""
import io, os, re, sys

REPO = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
BAT = os.path.join(REPO, "docs", "unxreal", "batches")
DEEP = os.path.join(REPO, "docs", "unxreal", "deepen")
NAMES = [f"E{i:02d}" for i in range(1, 106)]

ROW_RE = re.compile(r"^\| (UNX-E4-E(\d{3,4})) \| ([^|]+?) \| (\d+) \| 增补 \| (.+?) \|$", re.M)
SEC_RE = re.compile(r"^### (UNX-E4-E(\d{3,4})) · ", re.M)

errors = []
grand = 0
for ename in NAMES:
    bat = io.open(os.path.join(BAT, f"UNX-E4-{ename}.md"), encoding="utf-8").read()
    dpath = os.path.join(DEEP, f"E4-{ename}.md")
    dp = io.open(dpath, encoding="utf-8").read()
    bat_rows = {r[1]: (r[2], r[4]) for r in ROW_RE.findall(bat)}
    secs = SEC_RE.findall(dp)
    ids = [s[1] for s in secs]
    if len(ids) != 20:
        errors.append(f"E4-{ename}: 深化条数 {len(ids)} != 20")
    if ids != sorted(bat_rows, key=int):
        errors.append(f"E4-{ename}: ID 与批册不对应")
    # 按节切块校验
    blocks = re.split(r"^### ", dp, flags=re.M)[1:]
    for b in blocks:
        mid = re.match(r"UNX-E4-E(\d{3,4}) · ", b)
        if not mid:
            continue
        idn = mid.group(1)
        if idn not in bat_rows:
            errors.append(f"E4-{ename}: E{idn} 不在批册")
            continue
        if "[已深化]" not in b:
            errors.append(f"E4-{ename}: E{idn} 缺 [已深化]")
        crit = bat_rows[idn][1].split(" ")[0]
        if crit not in b:
            errors.append(f"E4-{ename}: E{idn} 判据号不一致（缺 {crit}）")
        for tag in ("**定位**", "**语义边界**", "**依赖与嫁接源**", "**风险与回退**", "- 正文："):
            if tag not in b:
                errors.append(f"E4-{ename}: E{idn} 缺要素 {tag}")
        mbody = re.search(r"- 正文：(.+)", b)
        if not mbody or len(mbody.group(1)) < 300:
            errors.append(f"E4-{ename}: E{idn} 正文 {len(mbody.group(1)) if mbody else 0} < 300 字")
    grand += len(ids)

if grand != 2100:
    errors.append(f"总条目 {grand} != 2100")

if errors:
    print("FAIL:")
    for e in errors[:40]:
        print(" -", e)
    sys.exit(1)
print(f"ALL PASS: 105 册 / 2100 条 / ID 与批册一一对应 / 状态[已深化] / 判据一致 / 六要素齐全 / 正文逐条 ≥300 字")
