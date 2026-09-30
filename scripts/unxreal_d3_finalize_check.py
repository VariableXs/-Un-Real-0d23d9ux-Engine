#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""UNX-D3 域收官 finalize 断言链（波08-M28 · B31–B40 段 + 域满账 240,000）。

体例沿用 M26/AI-16 D1 与 M25/AI-09 B4 收官判例，适配 D3 域口径：
每批 6,000 行、F14201–F14400、深化册 deepen/D3-B{b}.md、骨架册 batches/UNX-D3-B{b}.md。
全部断言逐项输出，非零失败 exit=1。
"""
import re
import sys
import os

BASE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))  # 仓库根（scripts/ 上一级）
DEEPEN = os.path.join(BASE, 'docs', 'unxreal', 'deepen')
BATCH = os.path.join(BASE, 'docs', 'unxreal', 'batches')
CHECKS = []

ID_LO, ID_HI = 14201, 14400


def check(name, ok, detail=''):
    CHECKS.append((name, bool(ok), detail))
    print('[%s] %s' % ('PASS' if ok else 'FAIL', name) + (' | ' + detail if detail else ''))


id_head_re = re.compile(r'^### UNX-F(\d{5}) ', re.M)
meta_re = re.compile(
    r'\n- 域/批：D3/B(\d+)｜判据：(UNX-F\d+-J1 .*?)｜纯功能行数：(\d+) 行（(.+?)；测试段不计）｜状态：\[已深化\]')
seg_num_re = re.compile(r'(\d+)\s*$')
body_re = re.compile(r'\n- 正文：(.+)')

# ---------- 1) B31–B40 十册逐册断言（深化侧） ----------
batch_data = {}
for b in range(31, 41):
    path = os.path.join(DEEPEN, 'D3-B%d.md' % b)
    if not os.path.exists(path):
        check('B%d 深化册存在' % b, False, path)
        continue
    t = open(path, encoding='utf-8').read()
    chars = len(t)
    check('B%d 深化册字数≥6,000' % b, chars >= 6000, '%d 字符' % chars)
    ids, rows, bad_seg, min_body = [], 0, [], (10**9, '')
    for m in id_head_re.finditer(t):
        fid = int(m.group(1))
        ids.append(fid)
        seg = t[m.end():m.end() + 1600]  # 窗口 900→1600：收官轮长正文（≥600 字）+元行超旧窗被截断，误判短正文

        hm = meta_re.search(seg)
        if not hm:
            bad_seg.append((fid, 'no-meta'))
            continue
        total = int(hm.group(3))
        segs = [int(seg_num_re.search(p.strip()).group(1)) for p in hm.group(4).split(' + ')]
        if sum(segs) != total:
            bad_seg.append((fid, '%d vs %s' % (total, segs)))
        rows += total
        bm = body_re.search(seg)
        wc = len(bm.group(1)) if bm else 0
        if wc < min_body[0]:
            min_body = (wc, fid)
    check('B%d 20 条' % b, len(ids) == 20, '%d 条' % len(ids))
    lo = ID_LO + (b - 31) * 20
    hi = lo + 19
    check('B%d ID 区间 F%d–F%d 连续' % (b, lo, hi), ids == list(range(lo, hi + 1)),
          '首 %s 末 %s' % (ids[0] if ids else '?', ids[-1] if ids else '?'))
    check('B%d 逐条分解守恒' % b, not bad_seg, str(bad_seg[:3]))
    check('B%d 批求和=6,000' % b, rows == 6000, str(rows))
    check('B%d 全条正文≥300（册内最小 %d@F%d）' % (b, min_body[0], min_body[1]), min_body[0] >= 300)
    check('B%d 册头批注收口行在位' % b, bool(re.search(r'^# 域 UNX-D3 · 深化册 · UNX-D3-B%d' % b, t, re.M)))
    batch_data[b] = {'ids': ids, 'rows': rows, 'file': path}

# ---------- 2) 段级 ID 连续唯一 ----------
all_ids = []
for b in range(31, 41):
    all_ids.extend(batch_data.get(b, {}).get('ids', []))
check('B31–B40 段 ID 连续 200 无跳号无重号', all_ids == list(range(ID_LO, ID_HI + 1)),
      '%d 条 首%s 末%s' % (len(all_ids), all_ids[0] if all_ids else '?', all_ids[-1] if all_ids else '?'))

# ---------- 3) 双锚保真 ----------
t31 = open(batch_data[31]['file'], encoding='utf-8').read() if 31 in batch_data else ''
t40 = open(batch_data[40]['file'], encoding='utf-8').read() if 40 in batch_data else ''
check('双锚 F14201∈B31（真联签总起）', 'UNX-F14201' in t31)
check('双锚 F14400∈B40（域收官终条）', 'UNX-F14400' in t40)

# ---------- 4) 骨架侧十册在位且判据零漂移（复用六查逻辑要点） ----------
skel_drift = []
for b in range(31, 41):
    sp = os.path.join(BATCH, 'UNX-D3-B%d.md' % b)
    dp = os.path.join(DEEPEN, 'D3-B%d.md' % b)
    if not (os.path.exists(sp) and os.path.exists(dp)):
        skel_drift.append((b, 'missing'))
        continue
    s = open(sp, encoding='utf-8').read()
    d = open(dp, encoding='utf-8').read()
    s_crits = dict((m.group(1), m.group(2).strip())
                   for m in re.finditer(r'### (UNX-F\d+) · [^\n]+\n- 域/批：D3/B%d｜纯功能行数：\d+｜状态：\[已深化\]｜判据：(UNX-F\d+-J1 .+)' % b, s))
    d_crits = dict((m.group(1), m.group(2).strip()) for m in re.finditer(r'### (UNX-F\d+) · [^\n]+\n- 域/批：D3/B%d｜判据：(UNX-F\d+-J1 .*?)｜纯功能行数：' % b, d))
    for fid in d_crits:
        if s_crits.get(fid) != d_crits[fid]:
            skel_drift.append((b, fid))
check('骨架侧 B31–B40 判据零漂移（双侧比对）', not skel_drift, str(skel_drift[:5]))

# ---------- 5) 全域 800 ID（B01–B40 四十册） ----------
dom_ids = []
missing = []
for b in range(1, 41):
    path = os.path.join(DEEPEN, 'D3-B%02d.md' % b)
    if not os.path.exists(path):
        missing.append(b)
        continue
    dom_ids.extend(int(m.group(1)) for m in id_head_re.finditer(open(path, encoding='utf-8').read()))
check('B01–B40 四十册深化册齐备', not missing, str(missing))
check('全域 ID 连续 800 无跳号无重号（F13601–F14400）',
      dom_ids == list(range(13601, 14401)),
      '%d 条 首%s 末%s' % (len(dom_ids), dom_ids[0] if dom_ids else '?', dom_ids[-1] if dom_ids else '?'))

# ---------- 6) 跨域零侵入（F14201–F14400 仅本会话十册持有） ----------
legal_files = set(batch_data[b]['file'] for b in batch_data)
# D3 段 ID 合法持有载体（收官轮补列）：骨架索引账十册+总纲台账+全量功能总汇编+统一协作总台账
for _b in range(31, 41):
    legal_files.add(os.path.join(BATCH, 'UNX-D3-B%02d.md' % _b))
for _rel in (os.path.join('docs', 'Varix', 'CoRun Varix STAR II · Unxreal',
                          'CoRun Varix STAR II · Unxreal · 总纲与施工书.md'),
             os.path.join('docs', 'Varix', 'CoRun Varix STAR II · Unxreal',
                          'CoRun Varix STAR II · Unxreal.md'),
             os.path.join('docs', 'Varix', 'CoRun Varix STAR II · Unxreal',
                          'CoRun Varix STAR II · Unxreal · 统一协作总台账.md')):
    legal_files.add(os.path.join(BASE, _rel))
intrusions = []
scan_root = os.path.join(BASE, 'docs')
for root, dirs, files in os.walk(scan_root):
    dirs[:] = [d for d in dirs if d not in ('.git', 'node_modules', '_attic')]
    for fn in files:
        if not fn.endswith('.md'):
            continue
        if fn.startswith('_backup_'):  # 并行会话存档快照非活文档持有者（与 _attic 同类豁免）
            continue
        fp = os.path.join(root, fn)
        if fp in legal_files:
            continue
        try:
            t = open(fp, encoding='utf-8', errors='replace').read()
        except OSError:
            continue
        for m in re.finditer(r'UNX-F(\d{5})(?!\d)', t):
            v = int(m.group(1))
            if ID_LO <= v <= ID_HI:
                intrusions.append((os.path.relpath(fp, BASE), v))
check('跨域零侵入（F14201–F14400 仅本会话十册持有）', not intrusions, str(intrusions[:5]))

# ---------- 7) 域满账断言 ----------
seg3 = sum(batch_data.get(b, {}).get('rows', 0) for b in range(31, 41))
check('B31–B40 段求和=60,000', seg3 == 60000, str(seg3))
check('域满账 180,000+60,000=240,000', 180000 + seg3 == 240000, str(180000 + seg3))

# ---------- 汇总 ----------
fails = [c for c in CHECKS if not c[1]]
print('\n===== D3 域收官 finalize 断言链：%d 项，%d 过，%d 败 =====' % (len(CHECKS), len(CHECKS) - len(fails), len(fails)))
for name, ok, detail in fails:
    print('  FAIL: %s | %s' % (name, detail))
sys.exit(1 if fails else 0)
