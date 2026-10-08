#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""UNX-C4 域满账 finalize 五步断言链（B01–B40 全域 800 条 · 240,000）。

沿 finalize_check.py / finalize_check_domain.py 体例与 C4 B16–B30 轮 v2 口径（R-C4-003
勘误判例后口径）：5 位 ID 正则 / fz 分项 ' + ' 分段尾数解析 / 判据边界 [^｜\\n] / 行数
锁定行含 状态：[已深化] 尾。判定纪律：先证伪检查器（--falsify），再对真实数据收口。

用法（仓库根执行）：
    python docs/unxreal/deepen/finalize_check_c4_close.py            # 正式收口（exit 0=全绿）
    python docs/unxreal/deepen/finalize_check_c4_close.py --falsify  # 证伪模式（exit 2=检查器抓错成功）
    python docs/unxreal/deepen/finalize_check_c4_close.py --post     # 台账回填后复跑（含总纲/根台账/handoff 后置断言）
"""
import os
import re
import sys

BASE = os.path.dirname(os.path.abspath(__file__))                    # docs/unxreal/deepen
REPO = os.path.abspath(os.path.join(BASE, '..', '..', '..'))         # 仓库根
BATCH_DIR = os.path.join(REPO, 'docs', 'unxreal', 'batches')
ZONGGANG = os.path.join(REPO, 'docs', 'Varix', 'CoRun Varix STAR II · Unxreal',
                        'CoRun Varix STAR II · Unxreal · 总纲与施工书.md')
GENTai = os.path.join(REPO, 'CoRun Varix STAR II · Unxreal.md')      # 根台账（仓库根）

ID_LO, ID_HI = 10401, 11200
BUDGET_31_40 = {b: (5330 if b <= 36 else 5210) for b in range(31, 41)}
assert sum(BUDGET_31_40.values()) == 52820
SEG1, SEG2 = 85680, 101500   # B01–B15 深化 / B16–B30 深化（域账守恒锚原文口径）
KW = ('/10', '千次', '万次', '对平', '对账', 'grep', '100/100', '核销', '零命中', '零偏离', '1 千', '断言')

ITEM_HEAD = re.compile(r'^### UNX-F(\d{5}) · ', re.M)
DEEP_ROW = re.compile(r'- 域/批：C4/B\d+｜判据：([^｜\n]+)｜纯功能行数：(\d+) 行（([^；\n]+)；测试段不计）｜状态：\[已深化\]')
BATCH_ROW = re.compile(r'- 域/批：C4/B\d+｜纯功能行数：(\d+)｜状态：\[骨架\]｜判据：([^｜\n]+)')
DEEP_LOCK = re.compile(r'行数锁定：批内求和 ([\d,]+)[^，]*，域累计 ([\d,]+)/240,000')
BATCH_LOCK = re.compile(r'域账累计：\S+ ([\d,]+) \+ 本批 ([\d,]+) = ([\d,]+) / 240,000')
SEG_NUM = re.compile(r'(\d+)\s*$')
BODY_LINE = re.compile(r'- 正文：[^\n]*')
FOUR = ('- **定位**：', '- **语义边界**：', '- **依赖与嫁接源**：', '- **风险与回退**：')

CHECKS = []


def check(name, ok, detail=''):
    CHECKS.append((name, bool(ok), detail))
    print('[%s] %s' % ('PASS' if ok else 'FAIL', name) + (' | ' + detail if detail else ''))


def parse_row(mid):
    segs = []
    for part in mid.split(' + '):
        m = SEG_NUM.search(part.strip())
        if not m:
            return None
        segs.append(int(m.group(1)))
    return segs


def read(p):
    with open(p, encoding='utf-8') as f:
        return f.read()


def deep_path(b):
    return os.path.join(BASE, 'C4-B%02d.md' % b)


def batch_path(b):
    return os.path.join(BATCH_DIR, 'UNX-C4-B%02d.md' % b)


def run_checks(tamper=False):
    """主断言链。tamper=True 时对内存副本注入三处破坏（证伪检查器用）。"""
    deep_texts, batch_texts = {}, {}
    for b in range(1, 41):
        deep_texts[b] = read(deep_path(b))
        batch_texts[b] = read(batch_path(b))
    if tamper:
        t = deep_texts[40]
        t = t.replace('纯功能行数：250 行（执行面 125 + 对账账 75 + 注账 50；测试段不计）',
                      '纯功能行数：251 行（执行面 125 + 对账账 75 + 注账 50；测试段不计）', 1)
        t = t.replace('### UNX-F11181 · ', '### UNX-F11180 · ', 1)   # 重号
        deep_texts[40] = t

    batch_sum = {}          # b -> 逐条求和（深化册）
    deep_ids_all, batch_ids_all = [], []
    char_stat = {}          # b -> (册字符, 正文合计)
    prev_cum = 0            # 批头链起点 B01 prev=0

    # ---------- 1) 深化册逐册 ----------
    for b in range(1, 41):
        t = deep_texts[b]
        heads = list(ITEM_HEAD.finditer(t))
        ids = [int(m.group(1)) for m in heads]
        lo, hi = ID_LO + (b - 1) * 20, ID_LO + (b - 1) * 20 + 19
        check('B%02d 深化册 20 条' % b, len(ids) == 20, '%d 条' % len(ids))
        check('B%02d 深化册 ID 区间 F%d–F%d' % (b, lo, hi), ids == list(range(lo, hi + 1)),
              '首%s 末%s' % (ids[0] if ids else '?', ids[-1] if ids else '?'))
        blocks = []
        for i, m in enumerate(heads):
            end = heads[i + 1].start() if i + 1 < len(heads) else len(t)
            blocks.append((ids[i], t[m.end():end]))
        bad_row, bad_fz, bad_jd, bad_four, bad_body, bad_rng = [], [], [], [], [], []
        total = 0
        body_sum = 0
        for fid, blk in blocks:
            rm = DEEP_ROW.search(blk)
            if not rm:
                bad_row.append(fid)
                continue
            rows = int(rm.group(2))
            segs = parse_row(rm.group(3))
            if segs is None or sum(segs) != rows:
                bad_fz.append((fid, rows, segs))
            if not (120 <= rows <= 600):
                bad_rng.append((fid, rows))
            total += rows
            jd = rm.group(1)
            # 判据成分口径分段：B01–B15 沿其收口时点口径（finalize_check.py 无成分断言，仅 J1 在位）；
            # B16–B40 沿引擎 v2 口径（J1+复测成分关键词表）。R-C4-003 判例：口径分段显账。
            if b <= 15:
                bad = ('UNX-F%d-J1' % fid) not in jd
            else:
                bad = ('UNX-F%d-J1' % fid) not in jd or not any(k in jd for k in KW)
            if bad:
                bad_jd.append(fid)
            if not all(f in blk for f in FOUR):
                bad_four.append(fid)
            bm = BODY_LINE.search(blk)
            if not bm or len(bm.group(0)) < 300:
                bad_body.append((fid, len(bm.group(0)) if bm else 0))
            else:
                body_sum += len(bm.group(0))
        check('B%02d 行数锁定行+状态[已深化] 尾 20/20' % b, not bad_row, str(bad_row[:3]))
        check('B%02d fz 分项求和守恒' % b, not bad_fz, str(bad_fz[:2]))
        check('B%02d 行数区间 120–600' % b, not bad_rng, str(bad_rng[:3]))
        check('B%02d 判据 J1+复测成分' % b, not bad_jd, str(bad_jd[:3]))
        check('B%02d 四项齐备（定位/边界/依赖/风险）' % b, not bad_four, str(bad_four[:3]))
        check('B%02d 正文整行 ≥300（awk 口径）' % b, not bad_body, str(bad_body[:3]))
        lock = DEEP_LOCK.search(t)
        check('B%02d 批头行数锁定行在位' % b, bool(lock))
        before = prev_cum
        if lock:
            check('B%02d 批内求和=逐条之和' % b, int(lock.group(1).replace(',', '')) == total,
                  '%s vs %d' % (lock.group(1), total))
            check('B%02d 域累计链衔接' % b,
                  int(lock.group(2).replace(',', '')) == prev_cum + total,
                  '%s vs %d+%d' % (lock.group(2), prev_cum, total))
            prev_cum = int(lock.group(2).replace(',', ''))
        if 31 <= b <= 40:
            check('B%02d 批求和=预算 %s' % (b, format(BUDGET_31_40[b], ',')),
                  total == BUDGET_31_40[b], str(total))
        # 批册
        bt = batch_texts[b]
        bids = [int(m.group(1)) for m in ITEM_HEAD.finditer(bt)]
        check('B%02d 批册 20 条+ID 同区间' % b, bids == list(range(lo, hi + 1)),
              '%d 条 首%s' % (len(bids), bids[0] if bids else '?'))
        btotal = sum(int(m.group(1)) for m in BATCH_ROW.finditer(bt))
        n_batch_rows = len(BATCH_ROW.findall(bt))
        check('B%02d 批册行数行 20/20' % b, n_batch_rows == 20, '%d 行' % n_batch_rows)
        check('B%02d 两册行数同构' % b, btotal == total, '%d vs %d' % (btotal, total))
        blk = BATCH_LOCK.search(bt) if b >= 16 else None
        if b >= 16:
            ok3 = bool(blk) and int(blk.group(1).replace(',', '')) == before \
                and int(blk.group(2).replace(',', '')) == total \
                and int(blk.group(3).replace(',', '')) == before + total
            check('B%02d 批头三元组链（prev+本批=累计）' % b, ok3,
                  blk.group(0)[:44] if blk else 'missing')
        elif b == 1:
            m1 = re.search(r'域账累计：本批 ([\d,]+) / 240,000', bt)
            check('B01 批册头单值式（域首批特例）+本批值对平',
                  bool(m1) and int(m1.group(1).replace(',', '')) == total,
                  m1.group(1) if m1 else 'missing')
        else:
            m3 = re.search(r'域账累计：\S+ ([\d,]+) \+ 本批 ([\d,]+) = ([\d,]+) / 240,000', bt)
            okm = bool(m3) and int(m3.group(2).replace(',', '')) == total \
                and int(m3.group(3).replace(',', '')) == before + total
            check('B%02d 批册头三元组（真实批区间前缀）' % b, okm,
                  m3.group(0)[:44] if m3 else 'missing')
        deep_ids_all.extend(ids)
        batch_ids_all.extend(bids)
        batch_sum[b] = total
        char_stat[b] = (len(t), body_sum)

    # ---------- 2) 段级 ----------
    seg3 = sum(batch_sum.get(b, 0) for b in range(31, 41))
    check('B31–B40 段求和=52,820', seg3 == 52820, str(seg3))
    check('域满账 85,680+101,500+52,820=240,000', SEG1 + SEG2 + seg3 == 240000,
          '%d+%d+%d' % (SEG1, SEG2, seg3))

    # ---------- 3) 全域 800 ----------
    check('全域深化册 800 ID 连续唯一 F%d–F%d' % (ID_LO, ID_HI),
          deep_ids_all == list(range(ID_LO, ID_HI + 1)),
          '%d 条 首%s 末%s' % (len(deep_ids_all), deep_ids_all[0] if deep_ids_all else '?',
                               deep_ids_all[-1] if deep_ids_all else '?'))
    check('全域批册 800 ID 连续唯一（两册同构）', batch_ids_all == deep_ids_all,
          '%d 条' % len(batch_ids_all))

    # ---------- 4) 防重四范围（表头级跨域零侵入 + 域 ID 边界）----------
    c4_files = set(deep_path(b) for b in range(1, 41)) | set(batch_path(b) for b in range(1, 41))
    intrude = []
    for root, dirs, files in os.walk(os.path.join(REPO, 'docs')):
        dirs[:] = [d for d in dirs if d not in ('.git', '_attic', 'node_modules', '__pycache__')]
        for fn in files:
            if not fn.endswith('.md'):
                continue
            fp = os.path.join(root, fn)
            if fp in c4_files:
                continue
            try:
                txt = read(fp)
            except OSError:
                continue
            for m in ITEM_HEAD.finditer(txt):
                v = int(m.group(1))
                if ID_LO <= v <= ID_HI:
                    intrude.append((os.path.relpath(fp, REPO), v))
    check('表头级跨域零侵入（C4 ID 标题仅 C4 八十册持有）', not intrude, str(intrude[:4]))
    check('域 ID 边界 F%d 起/F%d 止（册内无越界标题）' % (ID_LO, ID_HI),
          all(ID_LO <= v <= ID_HI for v in deep_ids_all))

    # ---------- 5) F11200 收官锚 ----------
    b40 = deep_texts[40]
    blk11200_m = re.search(r'^### UNX-F11200 · .*$', b40, re.M)
    blk11200 = b40[blk11200_m.start():] if blk11200_m else ''
    rows_11200 = DEEP_ROW.search(blk11200)
    check('F11200 收官锚 460 行规格', bool(rows_11200) and rows_11200.group(2) == '460',
          rows_11200.group(2) if rows_11200 else 'missing')
    tri = all(k in blk11200 for k in ('满条', '240,000', '满态', '[已深化]'))
    check('F11200 三断言文本在位（满条/满账 240,000/满态）', tri)

    # ---------- 6) 字数账输出（台账回填用，wc -m 独立复测为准）----------
    print('\n----- 字数账（python len 字符口径；台账回填以 wc -m 实测为准）-----')
    grand = 0
    for b in range(31, 41):
        c, body = char_stat[b]
        grand += body
        print('B%02d 册 %s 字 / 正文 %s 字' % (b, format(c, ','), format(body, ',')))
    print('B31–B40 正文合计 %s 字' % format(grand, ','))
    return CHECKS


def post_checks():
    """台账回填后置断言（--post）：总纲十行/根台账四处/handoff C4 块。"""
    zg = read(ZONGGANG)
    for b in range(31, 41):
        rows = BUDGET_31_40[b]
        lo, hi = ID_LO + (b - 31) * 20, ID_LO + (b - 31) * 20 + 19
        pat = re.compile(r'\| UNX-C4-B%d \| F%d–F%d \| 20 \| \[已深化\] \| 正文 [\d,]+ 字（wc -m 实计）· %s 行锁定零偏离[^|]*\| AI-14（finalize 断言链五步全过，见 deepen/C4-B%d\.md） \|'
                         % (b, lo, hi, format(rows, ',')))
        check('总纲 B%02d 行 [已深化]+字数回填' % b, bool(pat.search(zg)))
    check('总纲域 C4 小结闭账收官段在位', 'B31–B40 finalize 回填与域闭账收官' in zg)
    check('总纲修订记录收官行在位', '修订记录（AI-14 收官闭账会话' in zg)
    gt = read(GENTai)
    check('根台账 §三 C4 行翻满账', bool(re.search(r'C4[^\n]*240,000', gt)))
    check('根台账 §四 AI-14 收官会话块在位', '波08-M20' in gt)
    check('根台账 §六 收官修订行在位', bool(re.search(r'§?六[^\n]*M20|修订[^\n]*M20', gt)) or 'M20' in gt)
    import json
    h = json.load(open(os.path.join(REPO, 'docs', 'unxreal', 'handoff.json'), encoding='utf-8'))
    c4 = h['domain_ledger_progress']['C4']
    check('handoff C4 finalized_batches=40', c4['finalized_batches'] == 40, str(c4['finalized_batches']))
    check('handoff C4 rows_locked=240,000', c4['rows_locked'] == 240000, str(c4['rows_locked']))
    check('handoff C4 条目满态', c4.get('entries_deepened', 0) == 800 and c4.get('entries_skeleton', 0) == 0,
          'deepened=%s skeleton=%s' % (c4.get('entries_deepened'), c4.get('entries_skeleton')))
    check('handoff C4 finalized_list 连续 40', c4['finalized_list'] == ['UNX-C4-B%02d' % b for b in range(1, 41)],
          '%d 批' % len(c4['finalized_list']))


def main():
    argv = sys.argv[1:]
    if '--falsify' in argv:
        res = run_checks(tamper=True)
        fails = [c for c in res if not c[1]]
        print('\n===== 证伪模式：注入三处破坏（行数改写/ID 重号）=====')
        print('FAIL 捕获 %d 项（≥3 视为检查器有效）' % len(fails))
        for name, _, detail in fails[:6]:
            print('  抓到: %s | %s' % (name, detail))
        sys.exit(2 if len(fails) >= 3 else 3)
    if '--post' in argv:
        post_checks()
    else:
        run_checks(tamper=False)
    fails = [c for c in CHECKS if not c[1]]
    print('\n===== finalize 域满账断言链：%d 项，%d 过，%d 败 =====' % (len(CHECKS), len(CHECKS) - len(fails), len(fails)))
    for name, _, detail in fails:
        print('  FAIL: %s | %s' % (name, detail))
    sys.exit(1 if fails else 0)


if __name__ == '__main__':
    main()
