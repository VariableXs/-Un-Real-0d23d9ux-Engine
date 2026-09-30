# -*- coding: utf-8 -*-
"""UNX-I2 首产段骨架校验器（AI-42）· 六查：独立于生成器复跑文件系统真值。
1 批数=15  2 每批 20 条  3 ID 连续 F32801–F33100 零跳号  4 每批 6,000 行求和  5 判据 300 枚唯一  6 主汇编册卷对齐
"""
import os, re, sys, hashlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BATCH_DIR = os.path.join(ROOT, "docs", "unxreal", "batches")
MAIN = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")
BASE, N = 32801, 300

ent_re = re.compile(r"^### UNX-F(\d+) · (.+)$")
crit_re = re.compile(r"判据：UNX-F\d+-J1 ")

def main():
    fails = []
    all_ids, all_crits = [], []
    per_batch = {}
    for b in range(1, 16):
        p = os.path.join(BATCH_DIR, f"UNX-I2-B{b:02d}.md")
        if not os.path.exists(p):
            fails.append(f"missing {p}"); continue
        txt = open(p, encoding="utf-8").read()
        ents = [(int(m.group(1)), m.group(2)) for m in (ent_re.match(l) for l in txt.splitlines()) if m]
        crits = crit_re.findall(txt)
        # 行数提取
        lines = [int(m) for m in re.findall(r"纯功能行数：(\d+)", txt)]
        if len(ents) != 20: fails.append(f"B{b:02d}: {len(ents)} entries != 20")
        if len(crits) != 20: fails.append(f"B{b:02d}: {len(crits)} criteria != 20")
        if len(lines) != 20 or sum(lines) != 6000: fails.append(f"B{b:02d}: line sum {sum(lines) if lines else 0} != 6000")
        for i, _ in ents: all_ids.append(i)
        all_crits.extend(crits)
        per_batch[b] = (len(ents), sum(lines))
    # 3 连续
    exp = list(range(BASE, BASE + N))
    if all_ids != exp: fails.append(f"ID discontinuity: got {len(all_ids)} ids, first-mismatch={next((i for i,(a,b) in enumerate(zip(all_ids,exp)) if a!=b), None)}")
    # 5 判据唯一
    if len(set(all_crits)) != N: fails.append(f"criteria not unique: {len(set(all_crits))}/{N}")
    # 6 主汇编册卷对齐
    vol = open(MAIN, encoding="utf-8").read()
    for b in range(1, 16):
        ids0, ids1 = BASE + (b-1)*20, BASE + b*20 - 1
        if f"UNX-I2-B{b:02d}" not in vol: fails.append(f"main volume missing UNX-I2-B{b:02d}")
    cnt_main = len(re.findall(r"^### UNX-F3(28|29|30|31)\d\d · ", vol, re.M))
    if cnt_main < N: fails.append(f"main volume I2 entries {cnt_main} < 300")
    # 报告
    print(f"checks: batches={len(per_batch)} entries={len(all_ids)} line_total={sum(v[1] for v in per_batch.values())}")
    if fails:
        print("FAIL:"); [print(" -", f) for f in fails]; sys.exit(1)
    h = hashlib.sha256("\n".join(all_crits).encode()).hexdigest()[:16]
    print("SIX CHECKS ALL PASS exit=0 | criteria fingerprint:", h)

if __name__ == "__main__":
    main()
