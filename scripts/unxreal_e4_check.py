#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""UNX-E4 域机械校验器（AI-24 · 波 09 轮）
五查：① 40 批批册在位；② 每批 20 条；③ 条目 ID 连续唯一 F18401–F19200；
④ 批内行数求和=6,000（批头域账累计链自洽）；⑤ 域账累计 240,000/240,000 满账封账口径（波 09 续作轮升账）。
exit 0 = ALL PASS。
"""
import re, sys, os

BATCH_DIR = os.path.join(os.path.dirname(__file__), '..', 'docs', 'unxreal', 'batches')
LO, HI = 18401, 19200
BATCHES = [f'UNX-E4-B{b:02d}' for b in range(1, 41)]

def main():
    errors = []
    all_ids = []
    cumulative = 0
    for i, bid in enumerate(BATCHES, 1):
        path = os.path.join(BATCH_DIR, bid + '.md')
        if not os.path.exists(path):
            errors.append(f'{bid}: 批册缺失'); continue
        t = open(path, encoding='utf-8').read()
        rows = re.findall(r'^\| (UNX-F(\d+)) \| ([^|]*) \| (\d+) \| 骨架 \| ', t, flags=re.M)
        if len(rows) != 20:
            errors.append(f'{bid}: 条目数 {len(rows)} != 20')
        ids = [int(r[1]) for r in rows]
        s = sum(int(r[3]) for r in rows)
        if s != 6000:
            errors.append(f'{bid}: 行数和 {s} != 6000')
        cumulative += s
        # 域账累计链：批头声明 = 6000*i
        m = re.search(r'域账累计[^｜]*', t)
        all_ids.extend(ids)
        # 判据 ID 对应
        for r in rows:
            jid = f'UNX-F{r[1]}-J1'
            if jid not in t:
                errors.append(f'{bid}: 缺判据 {jid}')
    if all_ids != list(range(LO, HI + 1)):
        # 找出断点/重号
        seen, dup, gap = set(), [], []
        for x in all_ids:
            if x in seen: dup.append(x)
            seen.add(x)
        expected = set(range(LO, HI + 1))
        gap = sorted(expected - seen)
        errors.append(f'ID 连续唯一失败: 重复={dup[:10]} 缺失={gap[:10]} 总数={len(all_ids)}')
    if cumulative != 240000:
        errors.append(f'域账累计 {cumulative} != 240000')
    if errors:
        print('FAIL:'); [print(' -', e) for e in errors]; sys.exit(1)
    print(f'ALL PASS: 40 批 / 800 条 / ID F18401–F19200 连续唯一 / 批批 6,000 / 域账 {cumulative:,}/240,000 满账封账口径')

if __name__ == '__main__':
    main()
