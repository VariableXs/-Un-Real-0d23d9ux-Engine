# -*- coding: utf-8 -*-
"""B4 域深化册公共 writer：B31–B40 收官轮（fork 自 .gen_b4_deep_writer.py，修正 domain_cum 公式）。
体例：七段式（元行+定位+语义边界+依赖嫁接+风险回退+正文）。
校验：J1 前缀 / 行数与骨架全等 / 三段分解求和 / 正文≥300字 / 条数=20。
"""
import os, re

BASE = os.path.dirname(os.path.abspath(__file__))
SK_DIR = os.path.join(BASE, 'batches')
DP_DIR = os.path.join(BASE, 'deepen')

SK_RE = re.compile(
    r'### UNX-F(\d{4}) · (.+)\n- 域/批：B4/B\d+｜纯功能行数：(\d+)｜状态：\[骨架\]｜判据：(UNX-F\1-J1 .+)')

def load_sk_batch(idx):
    """读骨架册 20 条：fid/name/rows/judg（判据 verbatim）。"""
    path = os.path.join(SK_DIR, 'UNX-B4-B%02d.md' % idx)
    t = open(path, encoding='utf-8').read()
    out = []
    for m in SK_RE.finditer(t):
        out.append({'fid': int(m.group(1)), 'name': m.group(2).strip(),
                    'rows': int(m.group(3)), 'judg': m.group(4).strip()})
    assert len(out) == 20, ('load_sk_batch', idx, len(out))
    return out

def split_rows(total):
    """三段行数 38%/32%/30%（round），严格守恒。"""
    a = round(total * 0.38)
    b = round(total * 0.32)
    c = total - a - b
    assert a + b + c == total
    return a, b, c

def e(fid, loc, bnd, dep, risk, body, segs, kwin, win, retest):
    """条目构造器（九参数）。"""
    assert len(segs) == 3, (fid, segs)
    return {'fid': fid, 'loc': loc, 'bnd': bnd, 'dep': dep, 'risk': risk,
            'body': body, 'segs': segs, 'kwin': kwin, 'win': win, 'retest': retest}

def write_deep_batch(idx, entries, graft, gov):
    """生成深化册。theme 从骨架册头部引言行第三段自动提取。"""
    sk = load_sk_batch(idx)
    skmap = {s['fid']: s for s in sk}
    # 批主题：骨架册头部引言行
    sk_path = os.path.join(SK_DIR, 'UNX-B4-B%02d.md' % idx)
    sk_txt = open(sk_path, encoding='utf-8').read()
    m = re.search(r'^> AI-09 承办｜域账累计：[^｜]+｜(.+)$', sk_txt, re.M)
    assert m, 'theme line'
    theme = m.group(1).strip()

    assert len(entries) == 20, ('entries', idx, len(entries))
    byfid = {x['fid']: x for x in entries}
    assert set(byfid) == set(skmap), ('fid mismatch', idx,
        sorted(set(skmap) - set(byfid)), sorted(set(byfid) - set(skmap)))

    total_rows = sum(s['rows'] for s in sk)
    assert total_rows == 6000, ('rows', idx, total_rows)
    domain_cum = 180000 + 6000 * (idx - 30)  # 收官轮口径：B31=186,000 ... B40=240,000

    short, problems = [], []
    rendered = []
    for s in sk:
        fid, rows, judg = s['fid'], s['rows'], s['judg']
        x = byfid[fid]
        if not judg.startswith('UNX-F%d-J1' % fid):
            problems.append((fid, 'judg prefix'))
        a, b, c = split_rows(rows)
        body_full = (x['body'] + '。。与现存内核衔接点：' + x['kwin'] +
                     '。。与 Windows 对照：' + x['win'] +
                     '，，判据 UNX-F%d-J1 的复测方式：' % fid + x['retest'] + '。。')
        if len(body_full) < 300:
            short.append((fid, len(body_full)))
        for i, seg in enumerate([a, b, c]):
            if seg <= 0:
                problems.append((fid, 'seg%d' % i))
        meta = ('- 域/批：B4/B%d｜判据：%s｜纯功能行数：%d 行（%s %d + %s %d + %s %d；测试段不计）｜状态：[已深化]'
                % (idx, judg, rows, x['segs'][0], a, x['segs'][1], b, x['segs'][2], c))
        rendered.append('### UNX-F%d · %s\n%s\n- **定位**：%s\n- **语义边界**：%s\n'
                        '- **依赖与嫁接源**：%s\n- **风险与回退**：%s\n- 正文：%s'
                        % (fid, s['name'], meta, x['loc'], x['bnd'], x['dep'], x['risk'], body_full))
    if short:
        raise AssertionError('短正文: %r' % short)
    if problems:
        raise AssertionError('问题: %r' % problems)

    hdr = ('# 域 UNX-B4 · 深化册 · UNX-B4-B%d（F%d–F%d · 20 条 · 20 条新深化）\n\n'
           '> AI-09 承办｜本批 B%d [已深化] 收口：20 条全部为本会话新深化｜嫁接源：%s｜'
           '防重声明：与总纲 §7.3-B4 骨架条目逐条同名同判据同 ID，深化不改判据语义只补六要素与正文；'
           '批累计行数锁定 6,000，域累计 %s/240,000｜本批主题：%s｜四重治理件：%s\n\n'
           % (idx, sk[0]['fid'], sk[-1]['fid'], idx, graft,
              format(domain_cum, ','), theme, gov))
    out = hdr + '\n\n'.join(rendered) + '\n'
    os.makedirs(DP_DIR, exist_ok=True)
    path = os.path.join(DP_DIR, 'B4-B%02d.md' % idx)
    with open(path, 'w', encoding='utf-8', newline='\n') as f:
        f.write(out)
    print('B%02d OK: %s (%d chars, %d 条)' % (idx, path, len(out), len(rendered)))
