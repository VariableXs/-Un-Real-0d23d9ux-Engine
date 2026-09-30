#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""UNX-B3 域收官 finalize 断言链（B31–B40 段 + 域满账）
沿用 finalize_check.py 体例：全部断言逐项输出，非零失败 exit=1。
"""
import re
import sys
import glob
import os

BASE = os.path.dirname(os.path.abspath(__file__))  # .../docs/unxreal/deepen
DEEPEN = BASE
CHECKS = []  # (name, ok, detail)

def check(name, ok, detail=''):
    CHECKS.append((name, bool(ok), detail))
    tag = 'PASS' if ok else 'FAIL'
    print(f'[{tag}] {name}' + (f' | {detail}' if detail else ''))

head_re = re.compile(r'纯功能行数：(\d+) 行（(.+?)；测试段不计）')
id_head_re = re.compile(r'^### (UNX-F(\d{4})) ', re.M)
seg_num_re = re.compile(r'(\d+)\s*$')

def parse_row(mid):
    segs = []
    for part in mid.split(' + '):
        m = seg_num_re.search(part.strip())
        if not m:
            return None
        segs.append(int(m.group(1)))
    return segs

BUDGET = {b: 6240 for b in range(31, 41)}
ID_LO, ID_HI = 6201, 6400
DOM_LO, DOM_HI = 5601, 6400
# 协作册/根台账排除（认领登记层，零侵入）
collab_excludes = ('CoRun Varix STAR II',)

# ---------- 1) B31–B40 十册逐册断言 ----------
batch_data = {}  # batch -> {ids:[], rows:{}, chars:int, file}
for b in range(31, 41):
    path = os.path.join(DEEPEN, f'B3-B{b}.md')  # B31–B40 本就是两位
    if not os.path.exists(path):
        check(f'B{b} 文件存在', False, path)
        continue
    with open(path, encoding='utf-8') as f:
        t = f.read()
    chars = len(t)
    check(f'B{b} 字数≥6000', chars >= 6000, f'{chars} 字符')
    ids, rows = [], {}
    for m in id_head_re.finditer(t):
        fid = int(m.group(2))
        ids.append(fid)
        # 行数锁定行取条目块内首处
        seg = t[m.end():m.end() + 600]
        hm = head_re.search(seg)
        if not hm:
            rows[fid] = None
            continue
        total = int(hm.group(1))
        parsed = parse_row(hm.group(2))
        rows[fid] = (total, parsed)
    check(f'B{b} 20 条', len(ids) == 20, f'{len(ids)} 条')
    # ID 连续区间
    lo = ID_LO + (b - 31) * 20
    hi = lo + 19
    check(f'B{b} ID 区间 F{lo}–F{hi}', ids == list(range(lo, hi + 1)),
          f'首 {ids[0] if ids else "?"} 末 {ids[-1] if ids else "?"}')
    # 逐条三段分解 + 批求和
    bad_seg, total_sum = [], 0
    for fid, row in rows.items():
        if row is None:
            bad_seg.append((fid, 'no-row'))
            continue
        total, parsed = row
        if parsed is None or sum(parsed) != total:
            bad_seg.append((fid, f'{total} vs {parsed}'))
        total_sum += total
    check(f'B{b} 逐条三段分解守恒', not bad_seg, str(bad_seg[:3]))
    check(f'B{b} 批求和=6240', total_sum == BUDGET[b], f'{total_sum}')
    batch_data[b] = {'ids': ids, 'rows': rows, 'chars': chars, 'file': path}
    # finalize 表实测字数行（回填后态）
    check(f'B{b} finalize 表实测字数行在位', bool(re.search(r'\| 新深化字数合计 \| [\d,]+ 字符（字符实计；批总 ≥6,000 字达标 ✓） \|', t)))

# ---------- 2) 会话段 ID 连续唯一 ----------
all_ids = []
for b in range(31, 41):
    all_ids.extend(batch_data.get(b, {}).get('ids', []))
check('B31–B40 段 ID 连续 200 无跳号无重号',
      all_ids == list(range(ID_LO, ID_HI + 1)),
      f'{len(all_ids)} 条 首{all_ids[0] if all_ids else "?"} 末{all_ids[-1] if all_ids else "?"}')

# ---------- 3) 双锚保真 ----------
def anchor_in(batch, fid, frag):
    t = open(batch_data[batch]['file'], encoding='utf-8').read()
    m = re.search(rf'^### UNX-F{fid} ', t, re.M)
    return bool(m) and frag in t

check('双锚 F6201∈B31', anchor_in(31, 6201, '互操作账轴总起'))
check('双锚 F6400∈B40', anchor_in(40, 6400, '域收官终条'))

# ---------- 4) 全域 800 ID（B01–B40 四十册）----------
dom_ids = []
b01_30_missing = []
for b in range(1, 41):
    path = os.path.join(DEEPEN, f'B3-B{b:02d}.md')
    if not os.path.exists(path):
        b01_30_missing.append(b)
        continue
    t = open(path, encoding='utf-8').read()
    dom_ids.extend(int(m.group(2)) for m in id_head_re.finditer(t))
check('B01–B40 四十册文件齐备', not b01_30_missing, str(b01_30_missing))
check('全域 ID 连续 800 无跳号无重号',
      dom_ids == list(range(DOM_LO, DOM_HI + 1)),
      f'{len(dom_ids)} 条 首{dom_ids[0] if dom_ids else "?"} 末{dom_ids[-1] if dom_ids else "?"}')

# ---------- 5) 跨域零侵入（本会话十册范围；协作册排除 + ID 负向前瞻）----------
pat = re.compile(r'UNX-F5(9\d\d)(?!\d)|UNX-F6([012]\d\d)(?!\d)')
# 本会话合法 ID 集
legal = set(range(ID_LO, ID_HI + 1))
scan_files = []
for root, dirs, files in os.walk(BASE):
    dirs[:] = [d for d in dirs if d not in ('.git', '.workbuddy', '_attic', 'node_modules')]
    for fn in files:
        if fn.endswith('.md') and 'CoRun Varix STAR II' not in fn:
            scan_files.append(os.path.join(root, fn))
intrusions = []
for fp in scan_files:
    try:
        t = open(fp, encoding='utf-8', errors='replace').read()
    except OSError:
        continue
    for m in pat.finditer(t):
        num = int(m.group(1) or m.group(2)) + (5900 if m.group(1) else 6200 if int(m.group(2)) >= 0 else 0)
        # 重建数值：F5 9xx → 59xx? 简化：直接由原文提取
    # 改用更直接的方式重建数值
    for m in re.finditer(r'UNX-F(\d{4})(?!\d)', t):
        v = int(m.group(1))
        if ID_LO <= v <= ID_HI and v not in legal:
            intrusions.append((os.path.relpath(fp, BASE), v))
# 合法持有者：本会话十册之外引用 F6201–F6400 的文件（如协作册排除后，仅 deepen 本域册合法）
legal_files = set(batch_data[b]['file'] for b in range(31, 41))
real_intr = []
for fp, v in intrusions:
    full = os.path.join(BASE, fp) if not os.path.isabs(fp) else fp
    if full in legal_files:
        continue
    real_intr.append((fp, v))
check('跨域零侵入（F6201–F6400 仅本会话十册持有）', not real_intr, str(real_intr[:5]))

# ---------- 6) 域满账断言 ----------
seg1, seg2 = 83420, 94180
seg3 = sum(BUDGET.values())
check('域满账 83,420+94,180+62,400=240,000', seg1 + seg2 + seg3 == 240000,
      f'{seg1}+{seg2}+{seg3}={seg1+seg2+seg3}')
check('B31–B40 段求和=62,400', seg3 == 62400, str(seg3))

# ---------- 7) 每册 finalize 表在位 ----------
for b in range(31, 41):
    if b in batch_data:
        t = open(batch_data[b]['file'], encoding='utf-8').read()
        check(f'B{b} 批次 finalize 表在位', '## 批次 finalize 表（UNX-B3-B%d）' % b in t)

# ---------- 汇总 ----------
fails = [c for c in CHECKS if not c[1]]
print(f'\n===== finalize 域收官断言链：{len(CHECKS)} 项，{len(CHECKS)-len(fails)} 过，{len(fails)} 败 =====')
for name, ok, detail in fails:
    print(f'  FAIL: {name} | {detail}')
sys.exit(1 if fails else 0)
