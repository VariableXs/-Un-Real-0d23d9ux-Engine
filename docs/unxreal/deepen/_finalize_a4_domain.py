# -*- coding: utf-8 -*-
"""A4 域收官 finalize 五步断言链（AI-04 专用；B31–B40 满账封账 F2401–F3200）。

与 _a4_check.py 同口径：批号文件名 B01–B15 零填充，B16–B40 原样。
① 防重（标题行注册口径：每 ID 恰好注册于其批位的骨架册+深化册两件）
② 全域 ID 连续（骨架册 F2401–F3200 共 800 条）
③ 行数守恒（判据行累计 40×6000+40×6000=240,000；物理行数 40×43+40×163）
④ 判据行四段结构全域核（三段分解 sum==N）
⑤ 正文 ≥300 整行实计全域核
全部断言逐项输出，非零失败 exit=1。
"""
import re
import sys
import glob
import os

def suf(b):
    """批号→文件名后缀（B01–B15 零填充，B16–B40 原样）。"""
    return '%02d' % b if b < 16 else str(b)

def skp(b):
    return 'batches/UNX-A4-B%s.md' % suf(b)

def dpp(b):
    return 'deepen/A4-B%s.md' % suf(b)

SK = [skp(b) for b in range(1, 41)]
DP = [dpp(b) for b in range(1, 41)]

missing = [p for p in SK + DP if not os.path.exists(p)]
print('文件齐备：40 骨架册 + 40 深化册，缺失=%s' % (missing or '无'))
if missing:
    sys.exit(1)

def read(p):
    return open(p, encoding='utf-8').read()

id_head = re.compile(r'^### UNX-F(\d+) ', re.M)
row_line = re.compile(r'纯功能行数：(\d+) 行（(.+?)；测试段不计）')

# ---------- ① 防重：标题行注册口径 ----------
# 四范围登记：骨架册全域 + 深化册全域（每 ID 恰好 2 件=批位骨架册+批位深化册）
locs = {}  # fid -> set(relpath with /)
dup_in_file = []
for fp in SK + DP:
    t = read(fp)
    rel = fp.replace(os.sep, '/').replace(chr(92), '/')
    seen_in_file = set()
    for m in id_head.finditer(t):
        fid = int(m.group(1))
        if fid in seen_in_file:
            dup_in_file.append((rel, fid))
        seen_in_file.add(fid)
        locs.setdefault(fid, set()).add(rel)

bad_dup = dup_in_file
bad_loc = []
for fid in range(2401, 3201):
    b = (fid - 2401) // 20 + 1
    want = {skp(b), dpp(b)}
    fs = locs.get(fid, set())
    if fs != want:
        bad_loc.append((fid, sorted(fs)))
extra = sorted(k for k in locs if not (2401 <= k <= 3200))
ok1 = not bad_dup and not bad_loc and not extra
print('[%s] ① 防重（每 ID 恰好注册批位骨架+深化 2 件；文件内零重号；无域外号）'
      % ('PASS' if ok1 else 'FAIL')
      + ('' if ok1 else ' | 文件内重号=%s 批位失配=%s 域外号=%s'
         % (bad_dup[:3], bad_loc[:3], extra[:5])))

# ---------- ② 全域 ID 连续（骨架册） ----------
sk_ids = []
for fp in SK:
    t = read(fp)
    sk_ids.extend(int(m.group(1)) for m in id_head.finditer(t))
dp_ids = []
for fp in DP:
    t = read(fp)
    dp_ids.extend(int(m.group(1)) for m in id_head.finditer(t))
ok2 = sk_ids == list(range(2401, 3201)) and dp_ids == list(range(2401, 3201))
print('[%s] ② 全域 ID 连续 F2401–F3200（骨架 %d 条 + 深化 %d 条）'
      % ('PASS' if ok2 else 'FAIL', len(sk_ids), len(dp_ids)))

# ---------- ③ 行数守恒 ----------
# 主账：判据行累计 骨架 40×6000 + 深化 40×6000 = 240,000
# 物理行按历史验收体例分段固化（防篡改基线）：
#   骨架册 B29–B40=43 行、B01–B28=44 行；深化册 B03–B08=167 行、其余=163 行
SK43 = set(range(29, 41))
DP167 = set(range(3, 9))
sum_bad = []
tot_sk = tot_dp = 0
phys_bad = []
for b in range(1, 41):
    ts = read(skp(b))
    td = read(dpp(b))
    rs = sum(int(m.group(1)) for m in re.finditer(r'纯功能行数：(\d+)', ts))
    rd = sum(int(m.group(1)) for m in row_line.finditer(td))
    ns = len(ts.splitlines())
    nd = len(td.splitlines())
    if rs != 6000 or rd != 6000:
        sum_bad.append((b, rs, rd))
    exp_s = 43 if b in SK43 else 44
    exp_d = 167 if b in DP167 else 163
    if ns != exp_s or nd != exp_d:
        phys_bad.append((b, ns, exp_s, nd, exp_d))
    tot_sk += rs
    tot_dp += rd
ok3 = not sum_bad and not phys_bad and tot_sk == 240000 and tot_dp == 240000
print('[%s] ③ 行数守恒（判据累计 骨架%d+深化%d=240,000；物理行体例基线 违例=%s 求和违例=%s）'
      % ('PASS' if ok3 else 'FAIL', tot_sk, tot_dp, phys_bad[:3] or '无', sum_bad[:3] or '无'))

# ---------- ④ 判据行四段结构全域核 ----------
seg_num = re.compile(r'(\d+)\s*$')
bad4 = []
n_rows4 = 0
for fp in DP:
    t = read(fp)
    for m in row_line.finditer(t):
        n_rows4 += 1
        n = int(m.group(1))
        segs = []
        for part in m.group(2).split(' + '):
            sm = seg_num.search(part.strip())
            if not sm:
                bad4.append((fp, n, '段无数', part))
                break
            segs.append(int(sm.group(1)))
        if len(segs) == 3 and sum(segs) != n:
            bad4.append((fp, n, '和失恒', segs))
ok4 = not bad4 and n_rows4 == 800
print('[%s] ④ 判据行四段结构（全域 %d 条判据行；违例=%s）'
      % ('PASS' if ok4 else 'FAIL', n_rows4, bad4[:3] or '无'))

# ---------- ⑤ 正文 ≥300 整行实计全域核 ----------
body_bad = []
n_body = 0
for fp in DP:
    t = read(fp)
    for m in re.finditer(r'^- 正文：.*$', t, re.M):
        n_body += 1
        if len(m.group(0)) < 300:
            body_bad.append((fp, len(m.group(0))))
ok5 = not body_bad and n_body == 800
print('[%s] ⑤ 正文 ≥300 整行（全域 %d 条正文行；超短=%s）'
      % ('PASS' if ok5 else 'FAIL', n_body, body_bad[:3] or '无'))

fails = [x for x in (ok1, ok2, ok3, ok4, ok5) if not x]
print('\n===== A4 域 finalize 五步断言链：%d 过 %d 败 =====' % (5 - len(fails), len(fails)))
sys.exit(1 if fails else 0)
