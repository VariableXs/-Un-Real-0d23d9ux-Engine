# -*- coding: utf-8 -*-
"""A4 域批次双册结构校验器（AI-04 专用；与 AI-08 B3 域脚本无关）。"""
import re
import sys

B = sys.argv[1] if len(sys.argv) > 1 else None
BATCHES = [int(B)] if B else list(range(16, 31))
all_bad = []
for b in BATCHES:
    suf = '%02d' % b if b < 16 else str(b)
    skp = 'batches/UNX-A4-B%s.md' % suf
    dpp = 'deepen/A4-B%s.md' % suf
    try:
        sk = open(skp, encoding='utf-8').read()
        dp = open(dpp, encoding='utf-8').read()
    except FileNotFoundError as e:
        all_bad.append((skp, '缺文件', str(e)))
        continue
    ids_s = [int(m.group(1)) for m in re.finditer(r'### UNX-F(\d+) ·', sk)]
    ids_d = [int(m.group(1)) for m in re.finditer(r'### UNX-F(\d+) ·', dp)]
    rows_s = [int(m.group(1)) for m in re.finditer(r'纯功能行数：(\d+)', sk)]
    rows_d = [int(m.group(1)) for m in re.finditer(r'纯功能行数：(\d+) 行（', dp)]
    ok_ids = ids_s == ids_d and len(ids_s) == 20
    ok_rows = sum(rows_s) == 6000 and sum(rows_d) == 6000 and len(rows_s) == 20 and len(rows_d) == 20
    ok_seq = ids_s == list(range(ids_s[0], ids_s[0] + 20)) if ids_s else False
    # 判据编号一致性：判据行中首个 UNX-Fxxxx-Jn 必须等于条目注册 ID
    badj = []
    for name, t in (('sk', sk), ('dp', dp)):
        for m in re.finditer(r'### UNX-F(\d+) ·', t):
            fid = m.group(1)
            nxt = t.find('\n### UNX-F', m.end())
            seg = t[m.end():nxt if nxt != -1 else len(t)]
            for j in re.finditer(r'UNX-F(\d+)-J\d', seg):
                if j.group(1) != fid:
                    badj.append((name, fid, j.group(0)))
    # 行数分解和守恒（深化册）
    badseg = []
    for m in re.finditer(r'纯功能行数：(\d+) 行（(.+?)；测试段不计）', dp):
        n = int(m.group(1))
        segs = [int(re.search(r'(\d+)\s*$', p.strip()).group(1)) for p in m.group(2).split(' + ')]
        if sum(segs) != n:
            badseg.append((n, segs))
    # 正文 ≥300
    body = [len(m.group(0)) for m in re.finditer(r'^- 正文：.*$', dp, re.M)]
    nshort = sum(1 for x in body if x < 300)
    print('B%s: 条数%d/%d ID连续%s 行数%s+%s 分解违例%s 判据笔误%s 正文%d条最短%d 超短%d' % (
        suf, len(ids_s), len(ids_d), ok_seq and 'Y' or 'N', sum(rows_s), sum(rows_d),
        badseg or '无', badj or '无', len(body), min(body) if body else -1, nshort))
    if not (ok_ids and ok_rows and ok_seq and not badj and not badseg and nshort == 0):
        all_bad.append((skp, '结构违例', '见上'))
print('=== 结果:', '全绿' if not all_bad else all_bad)
sys.exit(1 if all_bad else 0)
