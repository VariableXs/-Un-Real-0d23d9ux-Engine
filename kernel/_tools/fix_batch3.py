#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""第三批收敛：把 check3.log 里的错误按簇批量修复。

簇 A：`alloc::format!` 直传/取址传给 `&'static str` detail —— 统一改为
       `alloc::format!(...).leak()`（E0308 & E0716 同根）。
簇 B：机械单项（逐文件精确替换，断言计数）。
"""
import os
import re
import sys

SRC = 'varix/src'
LOG = '_attic/check3.log'

# ---------------------------------------------------------------- 簇 A 站点提取
log = open(LOG, encoding='utf-8', errors='replace').read()
lines = log.split('\n')

sites = []  # (file, line)
i = 0
while i < len(lines):
    ln = lines[i]
    if ln.startswith('error[E0308]') or ln.startswith('error[E0716]'):
        blk = [lines[i]]
        j = i + 1
        while j < len(lines) and not lines[j].startswith(('error[', 'error: ', 'warning: ')):
            blk.append(lines[j])
            j += 1
        text = '\n'.join(blk)
        is_str = 'expected `&str`, found `String`' in text
        is_tmp = "creates a temporary value" in text and 'alloc::format!' in text
        if is_str or is_tmp:
            # 找所有 --> 行，取其下代码行含 alloc::format! 的那个
            k = 0
            while k < len(blk):
                lm = re.match(r'\s*--> (varix[\\/].+?):(\d+):\d+', blk[k])
                if lm and k + 2 < len(blk) and 'alloc::format!' in blk[k + 2]:
                    f = lm.group(1).replace('\\', '/').replace('varix/src/', '')
                    sites.append((f, int(lm.group(2))))
                    break
                k += 1
    i += 1

# 去重（cgb268/cgb243 是并行会话热文件，跳过）
sites = sorted(set(sites))
if '--dry' in sys.argv:
    for f, l in sites:
        print('DRY', f, l)
    print('TOTAL', len(sites))
    sys.exit(0)
SKIP = {'cgpu/cgb243_tarjan.rs', 'cgpu/cgb268_fuzz.rs'}
done = []
for f, l in sites:
    if f in SKIP:
        print('SKIP(热):', f, l)
        continue
    p = os.path.join(SRC, f)
    raw = open(p, encoding='utf-8', newline='').read()
    crlf = '\r\n' in raw
    s = raw.replace('\r\n', '\n')
    sl = s.split('\n')
    ln = sl[l - 1]
    pos = ln.find('alloc::format!(')
    assert pos >= 0, 'site 未找到 format!: %s:%d -> %r' % (f, l, ln[:120])
    # 若紧跟 & 前缀（&alloc::format!），移除（leak 直接产出 &str）
    strip_amp = pos > 0 and ln[pos - 1] == '&'
    # 从 format!( 的 ( 起做配对扫描（跳过字符串字面量与转义）
    d = pos + len('alloc::format!')
    assert ln[d] == '('
    depth = 0
    k = d
    n = len(s)
    while k < n:
        c = s[k]
        if c == '"':
            k += 1
            while k < n:
                if s[k] == '\\':
                    k += 2
                    continue
                if s[k] == '"':
                    break
                k += 1
        elif c == '(':
            depth += 1
        elif c == ')':
            depth -= 1
            if depth == 0:
                break
        k += 1
    assert depth == 0, '括号配对失败: %s:%d' % (f, l)
    # 插入 .leak()（含跨行：s 是整文件文本，行号 -> 偏移）
    off = sum(len(x) + 1 for x in sl[:l - 1])  # 该行起点
    ins = off + k + 1  # 闭括号后
    s = s[:ins] + '.leak()' + s[ins:]
    if strip_amp:
        amp = off + pos - 1
        assert s[amp] == '&'
        s = s[:amp] + s[amp + 1:]
    if crlf:
        s = s.replace('\n', '\r\n')
    open(p, 'w', encoding='utf-8', newline='').write(s)
    done.append((f, l))
print('leak 簇修复 %d 处' % len(done))
for f, l in done:
    print('  ', f, l)


# ---------------------------------------------------------------- 簇 B 机械项
def patch(rel, pairs):
    p = os.path.join(SRC, rel)
    raw = open(p, encoding='utf-8', newline='').read()
    crlf = '\r\n' in raw
    s = raw.replace('\r\n', '\n')
    for old, new, cnt in pairs:
        assert s.count(old) == cnt, '%s: %r 计数 %d != %d' % (rel, old[:60], s.count(old), cnt)
        s = s.replace(old, new)
    if crlf:
        s = s.replace('\n', '\r\n')
    open(p, 'w', encoding='utf-8', newline='').write(s)
    print('OK', rel, '(%d 项)' % len(pairs))


# svstar2（15:20-15:50 冷文件）
patch('svstar2/ves85_qualcert.rs', [
    ('.find(|l| self.threshold(*l)', '.find(|l| self.threshold(**l)', 1),
])
patch('svstar2/vr08d_cognia11y.rs', [
    ('#[derive(Clone, Debug)]\npub struct TimeGrace', '#[derive(Clone, Debug, Default)]\npub struct TimeGrace', 1),
])
patch('svstar2/vr08l_a11ydocs.rs', [
    ('''        let b = a.set_mut().book_mut(BookKind::Audit);
        b.present = true;
        b.bootstrap_ok = true;
        b.nav_depth = 3;
        out.add("0x8F04-超2跳拦", !a.set().nav_all_ok(), "");
        b.nav_depth = 2;
        out.add("0x8F04-2跳恰合规", a.set().nav_all_ok(), "");''',
     '''        {
            let b = a.set_mut().book_mut(BookKind::Audit);
            b.present = true;
            b.bootstrap_ok = true;
            b.nav_depth = 3;
        }
        out.add("0x8F04-超2跳拦", !a.set().nav_all_ok(), "");
        a.set_mut().book_mut(BookKind::Audit).nav_depth = 2;
        out.add("0x8F04-2跳恰合规", a.set().nav_all_ok(), "");''', 1),
])
patch('svstar2/vr09i_seccheck.rs', [
    ('''        e.closed = true;
        c.entries[0] = e;''', '''        c.entries[0].closed = true;''', 1),
])
patch('svstar2/vr09j_a11ycheck.rs', [
    ('''        e.level = WcagLevel::AA;
        c.entries[0] = e;''', '''        c.entries[0].level = WcagLevel::AA;''', 1),
])
patch('svstar2/vr09k_doccheck.rs', [
    ('''        e.bootstrap_ok = false;
        c.entries[0] = e;''', '''        c.entries[0].bootstrap_ok = false;''', 1),
])
patch('svstar2/ven09_fuzz.rs', [
    ('#[derive(Clone, Debug, Default, PartialEq, Eq)]\npub enum AttackVerdict',
     '#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]\npub enum AttackVerdict', 1),
])
patch('svstar2/mod.rs', [
    ('        ("VE-F2603", ven03_checks::run_ven03_all_checks),',
     '        ("VE-F2603-A", ven03_checks::run_ven03_checks_a),\n'
     '        ("VE-F2603-B", ven03_checks::run_ven03_checks_b),', 1),
])

# unxstar
patch('unxstar/unx252_crcchain.rs', [
    ('    let s = standard_payload();', '    let mut s = standard_payload();', 1),
])
patch('unxstar/unx253_vercompat.rs', [
    ('    pub fn read(&self, dom: usize) -> Result<Ver, u32> {',
     '    pub fn read(&self, dom: usize) -> Result<(), u32> {', 1),
])
patch('unxstar/unx257_fieldmap.rs', [
    ('        hit * 10_000 / self.rows.len()', '        (hit * 10_000 / self.rows.len()) as u32', 1),
])

# cgpu
patch('cgpu/cgb245_resolve.rs', [
    ('    pub fn select_victim(&self, scores: &[VictimScore]) -> Option<&VictimScore> {',
     "    pub fn select_victim<'a>(&self, scores: &'a [VictimScore]) -> Option<&'a VictimScore> {", 1),
])
patch('cgpu/cgb246_lockord.rs', [
    ('        v2.map(|v| v.severity == Severity::Red).unwrap_or(false),',
     '        v2.as_ref().map(|v| v.severity == Severity::Red).unwrap_or(false),', 1),
])
patch('cgpu/cgb277_frametele.rs', [
    ('mean > 12_000 &&', 'mean > 12_000.0 &&', 1),
])
patch('cgpu/cgb280_queryapi.rs', [
    ('            total_points: total,', '            total_points: total as usize,', 1),
    ('            query_time_us: self.estimate_query_time(total),',
     '            query_time_us: self.estimate_query_time(total as usize),', 1),
])
patch('cgpu/cgb286_apiv1.rs', [
    ('''    let samples: Vec<&TelemetrySample> = alloc::vec![
        &TelemetrySample { metric: "m".into(), timestamp_ns: 1, value: 10.0, frame: 1, task: 0, queue: 0, account: 0 },
        &TelemetrySample { metric: "m".into(), timestamp_ns: 2, value: 20.0, frame: 1, task: 0, queue: 0, account: 0 },
        &TelemetrySample { metric: "m".into(), timestamp_ns: 3, value: 30.0, frame: 2, task: 0, queue: 0, account: 0 },
    ];''',
     '''    let owned = [
        TelemetrySample { metric: "m".into(), timestamp_ns: 1, value: 10.0, frame: 1, task: 0, queue: 0, account: 0 },
        TelemetrySample { metric: "m".into(), timestamp_ns: 2, value: 20.0, frame: 1, task: 0, queue: 0, account: 0 },
        TelemetrySample { metric: "m".into(), timestamp_ns: 3, value: 30.0, frame: 2, task: 0, queue: 0, account: 0 },
    ];
    let samples: Vec<&TelemetrySample> = owned.iter().collect();''', 1),
])

# unx4801 族：补 ToString 导入
for rel in ['unxstar/unx4801_ext4tree.rs', 'unxstar/unx4841_dirents.rs',
            'unxstar/unx4881_xattrns.rs', 'unxstar/unx5061_fragmetric.rs',
            'unxstar/unx5381_subvol.rs', 'unxstar/unx6501_blkreg.rs']:
    p = os.path.join(SRC, rel)
    raw = open(p, encoding='utf-8', newline='').read()
    crlf = '\r\n' in raw
    s = raw.replace('\r\n', '\n')
    if re.search(r'use\s+alloc::string::\{[^}]*ToString', s) or 'use alloc::string::ToString;' in s:
        print('SKIP(已有):', rel)
        continue
    ls = s.split('\n')
    last_use = max(i for i, ln in enumerate(ls[:60]) if ln.startswith('use '))
    ls.insert(last_use + 1, 'use alloc::string::ToString;')
    s2 = '\n'.join(ls)
    if crlf:
        s2 = s2.replace('\n', '\r\n')
    open(p, 'w', encoding='utf-8', newline='').write(s2)
    print('OK', rel, '(ToString 导入)')
