# -*- coding: utf-8 -*-
# unxreal_e5_custom_check.py — E5 定制深化轮校验（800 条 × ≥300 字 / 40 册 / 号段覆盖 / 判据号对平）
import re, sys, io, os
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')
ok = True
def chk(n, c, d=''):
    global ok
    print(('PASS' if c else 'FAIL'), n, d)
    if not c: ok = False
CN = re.compile(r'[\u4e00-\u9fff]')
book_ids = {}
for b in range(1, 41):
    p = f'docs/unxreal/deepen/E5-C{b:02d}.md'
    if not os.path.exists(p):
        chk(f'册存在 C{b:02d}', False, p); continue
    s = open(p, encoding='utf-8').read()
    ids = re.findall(r'^### (UNX-F(\d{5})) · ', s, re.M)
    book_ids[b] = [i[0] for i in ids]
chk('1 册数40', len(book_ids) == 40, len(book_ids))
all_ids = sorted(int(v[0][5:]) for v in book_ids.values() for v2 in [v] for v2 in v) if False else sorted(int(x[5:]) for v in book_ids.values() for x in v)
chk('2 号段800连续', all_ids == list(range(19201, 20001)), len(all_ids))
# 3 每册 20 条
bad = [b for b, v in book_ids.items() if len(v) != 20]
chk('3 每册20条', not bad, str(bad))
# 4 每条定制正文 ≥300 汉字（以「定制正文：N 字」落账值复核 + 实测段长复核）
short = []; lied = []
for b, v in book_ids.items():
    s = open(f'docs/unxreal/deepen/E5-C{b:02d}.md', encoding='utf-8').read()
    blocks = re.split(r'^### ', s, flags=re.M)[1:]
    for blk in blocks:
        m = re.match(r'(UNX-F\d{5})', blk)
        if not m: continue
        body = blk.split('**功能语义定位**：', 1)[-1]
        real = len(CN.findall(body))
        m2 = re.search(r'定制正文：(\d+) 字', blk)
        if real < 300: short.append((m.group(1), real))
        if m2 and abs(int(m2.group(1)) - real) > 3: lied.append((m.group(1), m2.group(1), real))
chk('4 逐条≥300字', not short, str(short[:5]))
chk('5 字数落账属实', not lied, str(lied[:5]))
# 6 判据号对平：册内 J1 800 枚唯一且与号段一致
j1 = set()
for b in book_ids: j1 |= set(re.findall(r'UNX-F(\d{5})-J1', open(f'docs/unxreal/deepen/E5-C{b:02d}.md', encoding='utf-8').read()))
chk('6 J1对平800', len(j1) == 800 and sorted(int(x) for x in j1) == list(range(19201, 20001)), len(j1))
print('RESULT:', 'ALL PASS' if ok else 'HAS FAIL')
sys.exit(0 if ok else 1)
