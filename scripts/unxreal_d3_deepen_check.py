#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""UNX-D3 深化收口机械校验器（AI-18 · 波08-M11 扩容版）

六查（对应 finalize 五步断言链的机械化证据）：
  1. entries   —— 深化册 600 条（30 册 × 20 条），判据 ID UNX-F13601-J1…UNX-F14200-J1 全域唯一且自指
  2. rows      —— 行数守恒：逐条纯功能行数求和 = 180,000；逐批 = 6,000；深化册与骨架索引账两侧一致
  3. drift     —— 深化册元行判据 vs 骨架索引账判据逐字比对（必须零漂移）
  4. short     —— 逐条正文（- 正文： 单行）实计字数 ≥300
  5. skeleton  —— 骨架索引账 30 件 × 20 条与深化册 ID 一一对应（双向无遗漏）
  6. words     —— 逐册正文合计字数（body-only 口径）输出，供总纲 §7.3-D3 与 handoff 台账回填

用法：python scripts/unxreal_d3_deepen_check.py
退出码：0 = 全绿；1 = 存在 PROBLEMS。
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BATCH = ROOT / 'docs' / 'unxreal' / 'batches'
DEEPEN = ROOT / 'docs' / 'unxreal' / 'deepen'
BODY_MIN = 300


def main():
    problems = []

    # ---------- 骨架索引账（batches/UNX-D3-B01..B30.md） ----------
    skel = {}  # fid -> (batch, rows, criteria)
    for b in range(1, 31):
        p = BATCH / ('UNX-D3-B%02d.md' % b)
        if not p.exists():
            problems.append('骨架索引账缺失: %s' % p.name)
            continue
        txt = p.read_text(encoding='utf-8')
        n = 0
        rows = 0
        for m in re.finditer(
                r'### (UNX-F\d+) · .*?\n- 域/批：D3/B(\d+)｜纯功能行数：(\d+)｜状态：\[(?:骨架|已深化)\]｜判据：(UNX-F\d+-J1 .+)', txt):
            fid, bb, r, crit = m.group(1), int(m.group(2)), int(m.group(3)), m.group(4).strip()
            if bb != b:
                problems.append('%s: %s 元行批号 B%02d 与册号不符' % (p.name, fid, bb))
            if fid in skel:
                problems.append('骨架判据 ID 重复: %s' % fid)
            skel[fid] = (b, r, crit)
            n += 1
            rows += r
        if n != 20:
            problems.append('%s: 骨架条目数 %d != 20' % (p.name, n))
        if rows != 6000:
            problems.append('%s: 骨架行数和 %d != 6000' % (p.name, rows))

    # ---------- 深化册（deepen/D3-B01..B30.md） ----------
    deep = {}  # fid -> (batch, rows, criteria)
    words = {}  # batch -> (word_sum, rows, (min_len, min_fid))
    total_rows = 0
    short_cnt = 0
    for b in range(1, 31):
        p = DEEPEN / ('D3-B%02d.md' % b)
        if not p.exists():
            problems.append('深化册缺失: %s' % p.name)
            continue
        txt = p.read_text(encoding='utf-8')
        blocks = re.split(r'(?=\n### UNX-F)', txt)
        n = 0
        rows = 0
        wsum = 0
        minlen = (10 ** 9, '')
        for blk in blocks:
            m = re.match(r'\n?### (UNX-F\d+) · ', blk)
            if not m:
                continue
            fid = m.group(1)
            mm = re.search(
                r'\n- 域/批：D3/B(\d+)｜判据：(UNX-F\d+-J1 .*?)｜纯功能行数：(\d+) 行（.*?）｜状态：\[已深化\]', blk)
            if not mm:
                problems.append('%s: %s 元行格式不符或状态非[已深化]' % (p.name, fid))
                continue
            bb, crit, r = int(mm.group(1)), mm.group(2), int(mm.group(3))
            if bb != b:
                problems.append('%s: %s 元行批号 B%02d 与册号不符' % (p.name, fid, bb))
            if fid in deep:
                problems.append('深化判据 ID 重复: %s' % fid)
            if not crit.startswith(fid + '-J1'):
                problems.append('%s: %s 判据号不自指（应为 %s-J1）' % (p.name, fid, fid))
            bm = re.search(r'\n- 正文：(.+)', blk)
            if not bm:
                problems.append('%s: %s 缺正文行' % (p.name, fid))
                wc = 0
            else:
                wc = len(bm.group(1))
            if wc < BODY_MIN:
                short_cnt += 1
                problems.append('%s: %s 正文 %d 字 < %d' % (p.name, fid, wc, BODY_MIN))
            if wc < minlen[0]:
                minlen = (wc, fid)
            wsum += wc
            deep[fid] = (b, r, crit)
            n += 1
            rows += r
        if n != 20:
            problems.append('%s: 深化条目数 %d != 20' % (p.name, n))
        if rows != 6000:
            problems.append('%s: 深化行数和 %d != 6000' % (p.name, rows))
        words[b] = (wsum, rows, minlen)
        total_rows += rows

    # ---------- drift：深化 vs 骨架 逐字比对 ----------
    drift = 0
    for fid in sorted(deep):
        b, r, crit = deep[fid]
        if fid not in skel:
            problems.append('深化条 %s 无对应骨架条目' % fid)
            continue
        sb, sr, scrit = skel[fid]
        if crit != scrit:
            drift += 1
            problems.append('判据漂移: %s' % fid)
        if r != sr:
            problems.append('行数漂移: %s 深化 %d vs 骨架 %d' % (fid, r, sr))
    for fid in sorted(skel):
        if fid not in deep:
            problems.append('骨架条 %s 无对应深化条目' % fid)

    # ---------- ID 连续性：F13601–F14200 ----------
    expect = set('UNX-F%d' % i for i in range(13601, 14201))
    got = set(deep) | set(skel)
    if got != expect:
        problems.append('ID 面不等于 F13601–F14200：多 %s 缺 %s'
                        % (sorted(got - expect)[:5], sorted(expect - got)[:5]))

    # ---------- 报告 ----------
    total_words = sum(w[0] for w in words.values())
    gmin = min((w[2] for w in words.values()), default=(0, ''))
    print('entries = %d' % len(deep))
    print('rows    = %d' % total_rows)
    print('drift   = %d' % drift)
    print('short   = %d' % short_cnt)
    print('PROBLEMS: %d' % len(problems))
    for x in problems:
        print('  - %s' % x)
    print('--- 逐册正文合计字数（body-only）与行数 ---')
    for b in range(1, 31):
        if b in words:
            w, r, mn = words[b]
            print('B%02d: words=%d rows=%d min=%d(%s)' % (b, w, r, mn[0], mn[1]))
    print('TOTAL words = %d' % total_words)
    print('GLOBAL MIN = %d (%s) => ALL >= %d: %s' % (gmin[0], gmin[1], BODY_MIN, gmin[0] >= BODY_MIN))
    return 0 if not problems else 1


if __name__ == '__main__':
    sys.exit(main())
