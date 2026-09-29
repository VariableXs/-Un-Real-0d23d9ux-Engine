# -*- coding: utf-8 -*-
"""UNX-B3 域 B16-B30 finalize 断言链（AI-08 专用）。
断言范围：行数守恒 / 锚点保真 / ID 连续唯一 / 字数阈值 / 跨域零侵入。
口径声明：字数 = 整文件 len()（回填前冻结时点）；行数 = 各条 bullet 声明值逐段求和。
解析器说明：行数行为「标签 N + 标签 N + 标签 N；测试段不计」体例（标签可缺省、
段数以 3 段为主，B01–B15 存在 8 条四段式历史条目，均按「 + 」切分取段尾数字求和）。
跨域零侵入说明：共享协作册（仓库根台账 + docs/Varix 协作四件套）为全域认领登记层，
本域认领表/任务书示例锚在其中合法登记本域 ID，故排除出扫描；他域深化册与 kernel
对 F5901–F6200 的引用仍为红线（ID 边界用负向前瞻，排除他域五位数 ID 前缀误报）。
"""
import re
import sys
import glob
import os

os.chdir(os.path.dirname(os.path.abspath(__file__)))

DECL = {16: 6400, 17: 6300, 18: 6300, 19: 6400, 20: 6200, 21: 6300, 22: 6200,
        23: 6200, 24: 6300, 25: 6200, 26: 6600, 27: 6200, 28: 6200, 29: 6180, 30: 6200}

ent_re = re.compile(r'### UNX-F(\d+) · (.+)')
head_re = re.compile(r'纯功能行数：(\d+) 行（(.+?)；测试段不计）')


def parse_row(mid):
    """「标签 N + 标签 N + 标签 N」→ 段值列表；段尾数字为段行数。"""
    segs = []
    for part in mid.split(' + '):
        m = re.search(r'(\d+)\s*$', part.strip())
        if not m:
            return None
        segs.append(int(m.group(1)))
    return segs


ok, fail = [], []


def chk(name, cond, detail):
    (ok if cond else fail).append('%s: %s' % (name, detail))


all_ids = {}
chars = {}


def scan_batch(path, prefix):
    """返回 (行数声明总和, 行数行条数, 条目数, 违例列表)；违例=(N, segs)。"""
    t = open(path, encoding='utf-8').read()
    ids = [int(m.group(1)) for m in ent_re.finditer(t)]
    for i in ids:
        all_ids.setdefault(i, []).append(prefix)
    total, nrow, bad = 0, 0, []
    for m in head_re.finditer(t):
        nrow += 1
        n = int(m.group(1))
        segs = parse_row(m.group(2))
        if segs is None or sum(segs) != n:
            bad.append((n, segs))
        else:
            total += n
    return total, nrow, len(ids), bad, t


# ---- B01-B15 段求和（含逐条守恒复验） ----
s15 = 0
for b in range(1, 16):
    total, nrow, nent, bad, _ = scan_batch('B3-B%02d.md' % b, b)
    s15 += total
    chk('B%02d 分段和守恒' % b, not bad, '%d 条零违例' % nrow if not bad else '违例%s' % bad)
    chk('B%02d 行数行=条目数' % b, nrow == nent, '%d/%d' % (nrow, nent))
chk('B01-B15 求和=83420', s15 == 83420, str(s15))

# ---- B16-B30 段 ----
s1630 = 0
for b in range(16, 31):
    total, nrow, nent, bad, t = scan_batch('B3-B%d.md' % b, b)
    chars[b] = len(t)
    s1630 += total
    chk('B%d 求和=%d' % (b, DECL[b]), total == DECL[b], str(total))
    chk('B%d 条目数=20' % b, nent == 20 and nrow == 20, '条目%d/行数行%d' % (nent, nrow))
    chk('B%d 分段和守恒' % b, not bad, '零违例' if not bad else '违例%s' % bad)

chk('域累计=177600', s15 + s1630 == 177600, '%d+%d=%d' % (s15, s1630, s15 + s1630))

# ---- 锚点 ----
m = re.search(r'UNX-F5920 ·.*?纯功能行数：(\d+)', open('B3-B16.md', encoding='utf-8').read(), re.S)
chk('锚 F5920=420', bool(m) and m.group(1) == '420', m.group(1) if m else '缺')
m = re.search(r'UNX-F6010 ·.*?纯功能行数：(\d+)', open('B3-B21.md', encoding='utf-8').read(), re.S)
chk('锚 F6010=280', bool(m) and m.group(1) == '280', m.group(1) if m else '缺')

# ---- ID 连续唯一 ----
want = list(range(5601, 6201))
have = sorted(all_ids)
sym = sorted(set(want) ^ set(have))
chk('ID 集合=F5601..F6200', have == want, '%d 个, 差集 %s' % (len(have), sym or '无'))
dup = {k: v for k, v in all_ids.items() if len(v) > 1}
chk('ID 零重号', not dup, '唯一' if not dup else str(dup))

# ---- 字数阈值 ----
for b in range(16, 31):
    chk('B%d 字数≥6000' % b, chars[b] >= 6000, str(chars[b]))

# ---- 跨域零侵入：他域深化册 + kernel 对 F5901-F6200 零引用 ----
# ID 负向前瞻：UNX-F5920 匹配而 UNX-F59201（他域五位数 ID）不匹配
pat = re.compile(r'UNX-F5(9\d\d)(?!\d)|UNX-F6([012]\d\d)(?!\d)')
intruders = []
for f in glob.glob(os.path.join('..', '..', '**', '*.md'), recursive=True):
    fn = os.path.basename(f)
    if fn.startswith('B3-'):
        continue
    if 'CoRun Varix STAR II' in f:
        continue  # 共享协作册（根台账 + 协作四件套）：全域认领登记层，本域认领合法在册
    try:
        t = open(f, encoding='utf-8').read()
    except Exception:
        continue
    hits = pat.findall(t)
    if hits:
        intruders.append((os.path.relpath(f, '..'), len(hits)))
chk('跨域零侵入(md)', not intruders, '零命中' if not intruders else str(intruders[:10]))

kern_hits = []
for dirpath, _, files in os.walk(os.path.join('..', '..', 'kernel')):
    for f in files:
        if f.endswith(('.rs', '.toml', '.md')):
            p = os.path.join(dirpath, f)
            try:
                t = open(p, encoding='utf-8').read()
            except Exception:
                continue
            if pat.search(t):
                kern_hits.append(os.path.relpath(p, '..'))
chk('kernel 对 F59xx-F62xx 零引用', not kern_hits, '零命中' if not kern_hits else str(kern_hits[:10]))

# ---- 输出 ----
print('=== 通过 %d 项 ===' % len(ok))
for l in ok:
    print(' OK', l)
if fail:
    print('=== 失败 %d 项 ===' % len(fail))
    for l in fail:
        print(' XX', l)
    sys.exit(1)
print()
print('=== B16-B30 冻结字数（整文件 len，回填前时点）===')
for b in range(16, 31):
    print('B%d = %d' % (b, chars[b]))
print('会话段字数合计 = %d' % sum(chars.values()))
