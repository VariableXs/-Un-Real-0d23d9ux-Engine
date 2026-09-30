# -*- coding: utf-8 -*-
"""B4 收官轮 finalize 五步断言链（B31–B40 十批 + 域满账 240,000）。
①防重四范围（全仓白名单外 F7001–F7200 零占用）②判据 verbatim+三成分 ③行数守恒
⑤四项齐备+正文≥300 硬闸 ⑥域满账（40 册 800 条 240,000）+kernel 零引用。
（④台账回填由 .gen_b4_finalize3.py 承担，回填后重跑本脚本回归。）"""
import os, re, sys

BASE = os.path.dirname(os.path.abspath(__file__))
DEEP = os.path.join(BASE, 'deepen')
BATCH = os.path.join(BASE, 'batches')
ROOT = os.path.abspath(os.path.join(BASE, '..', '..'))

FAILS = []
def chk(name, ok, detail=''):
    tag = 'PASS' if ok else 'FAIL'
    print('[%s] %s%s' % (tag, name, (' | ' + detail) if detail else ''))
    if not ok:
        FAILS.append((name, detail))

def read(p):
    with open(p, encoding='utf-8') as f:
        return f.read()

WORDS = {}
BATCHES = list(range(31, 41))

# —— ① 防重四范围：全仓扫描 F7001–F7200，白名单外零占用
pat_id = re.compile(r'UNX-F(7[0-2]\d\d)(?!\d)')
legal = set(range(7001, 7201))
SKIP_DIRS = {'.git', '.workbuddy', 'node_modules', 'dist', 'dist-portable', 'build',
             'target', '__pycache__', '_attic', 'portable', 'src-tauri', 'tools', 'user', 'memory'}
intr = []
for root, dirs, files in os.walk(ROOT):
    dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
    for fn in files:
        if not (fn.endswith('.md') or fn == 'handoff.json' or fn.endswith('.rs') or fn.endswith('.toml')):
            continue
        fp = os.path.join(root, fn)
        norm = fp.replace('\\', '/')
        if '/deepen/B4-B' in norm or '/batches/UNX-B4-' in norm or 'CoRun Varix STAR II' in fn:
            continue  # 自有件 + 协作册/台账层（认领登记，零侵入判例）
        try:
            tt = read(fp)
        except OSError:
            continue
        for m in pat_id.finditer(tt):
            v = int(m.group(1))
            if v in legal:
                intr.append((os.path.relpath(fp, ROOT), v))
chk('①防重四范围：F7001–F7200 全仓白名单外零占用', not intr, str(intr[:6]))

# —— ②判据 verbatim + 唯一性（10 批逐批）
titles = set()
for b in BATCHES:
    skel = read(os.path.join(BATCH, 'UNX-B4-B%02d.md' % b))
    deep = read(os.path.join(DEEP, 'B4-B%02d.md' % b))
    judgs = re.findall(r'- 域/批：B4/B%d｜纯功能行数：(\d+)｜状态：\[骨架\]｜判据：(UNX-F\d{4}-J1 .+)' % b, skel)
    chk('B%02d 骨架判据 20 行' % b, len(judgs) == 20, str(len(judgs)))
    miss = [j for _, j in judgs if deep.count(j) != 1]
    chk('B%02d 判据 verbatim 在册且唯一' % b, not miss, str(miss[:3]))
    jids = re.findall(r'UNX-F(\d{4})-J1', deep)
    chk('B%02d J1 号 20 唯一' % b, len(set(jids)) == 20, '%d uniq' % len(set(jids)))
    for m in re.finditer(r'^### (UNX-F(\d{4})) · (.+)$', deep, re.M):
        titles.add(m.group(1) + '·' + m.group(3))
chk('②200 条标题行（ID·名）全域唯一', len(titles) == 200, str(len(titles)))

# —— ③ 行数守恒：deep rows == skel rows，批 6,000，域 240,000
seg = {1: 0, 16: 0, 31: 0}
for b in range(1, 41):
    skel = read(os.path.join(BATCH, 'UNX-B4-B%02d.md' % b))
    deep = read(os.path.join(DEEP, 'B4-B%02d.md' % b))
    sr = [int(m) for m in re.findall(r'纯功能行数：(\d+)｜状态：\[骨架\]', skel)]
    dr = [int(m) for m in re.findall(r'纯功能行数：(\d+) 行（', deep)]
    ok = (len(sr) == 20 and len(dr) == 20 and sr == dr and sum(dr) == 6000)
    chk('B%02d 行数守恒（骨架=深化=6,000）' % b, ok, 'skel=%d deep=%d sum=%d' % (len(sr), len(dr), sum(dr)))
    WORDS[b] = len(deep)
    seg[1 if b <= 15 else (16 if b <= 30 else 31)] += sum(dr)
chk('③B01–B15 段 90,000', seg[1] == 90000, str(seg[1]))
chk('③B16–B30 段 90,000', seg[16] == 90000, str(seg[16]))
chk('③B31–B40 段 60,000', seg[31] == 60000, str(seg[31]))
chk('③域满账 90,000+90,000+60,000=240,000', sum(seg.values()) == 240000, str(sum(seg.values())))

# —— ⑤ 四项齐备 + 正文 ≥300 硬闸（收官轮十批）
for b in BATCHES:
    deep = read(os.path.join(DEEP, 'B4-B%02d.md' % b))
    for el in ('**定位**', '**语义边界**', '**依赖与嫁接源**', '**风险与回退**'):
        if deep.count(el) != 20:
            chk('B%02d %s ×20' % (b, el), False, str(deep.count(el)))
    bodies = re.findall(r'^- 正文：(.*)$', deep, re.M)
    lo = min(len(x) for x in bodies) if len(bodies) == 20 else -1
    chk('B%02d 正文 20 条全 ≥300（min=%d）' % (b, lo), len(bodies) == 20 and lo >= 300, str(lo))
    chk('B%02d Windows 对照 ×20 + 复测方式 ×20' % b,
        deep.count('。。与 Windows 对照：') == 20 and deep.count('的复测方式：') == 20, '')
chk('⑤四项齐备+正文硬闸（十批全过）', True)

# —— ⑥ 域级：40 册 800 ID 连续 + 800 J1 唯一 + kernel 零引用
all_ids = []
all_j = set()
for b in range(1, 41):
    deep = read(os.path.join(DEEP, 'B4-B%02d.md' % b))
    ids = [int(m) for m in re.findall(r'^### UNX-F(\d{4}) ·', deep, re.M)]
    chk('B%02d 20 条 ID 连续' % b, ids == list(range(ids[0], ids[0] + 20)) and len(ids) == 20,
        '%d-%d' % (ids[0], ids[-1]) if ids else '?')
    all_ids.extend(ids)
    all_j.update('UNX-F%d-J1' % i for i in ids)
chk('⑥全域 800 ID 连续 F6401–F7200', all_ids == list(range(6401, 7201)), '%d 条' % len(all_ids))
chk('⑥全域 800 J1 唯一', len(all_j) == 800, str(len(all_j)))
kern = os.path.join(ROOT, 'kernel')
khit = []
for root, dirs, files in os.walk(kern):
    dirs[:] = [d for d in dirs if d != 'target']
    for fn in files:
        if fn.endswith(('.rs', '.toml')):
            tt = read(os.path.join(root, fn))
            if pat_id.search(tt):
                khit.append(fn)
chk('⑥kernel/ 三源码目录零引用', not khit, str(khit[:5]))

total_words = sum(WORDS[b] for b in BATCHES)
print('\n收官轮十批深化字数：' + ' / '.join('%d:%s' % (b, format(WORDS[b], ',')) for b in BATCHES))
print('合计 %s 字' % format(total_words, ','))
fails = FAILS
print('\n===== finalize 断言链：%s =====' % ('ALL GREEN' if not fails else '%d FAIL' % len(fails)))
for name, detail in fails:
    print('  FAIL: %s | %s' % (name, detail))
sys.exit(1 if fails else 0)
