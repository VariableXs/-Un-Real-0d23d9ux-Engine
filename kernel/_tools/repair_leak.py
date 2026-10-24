#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""修复 fix_batch3.py 同文件多站点偏移错插：清除全部 .leak() 后按降行序重插。"""
import os
import re
import sys

SRC = 'varix/src'

# 与 dry-run 相同的站点清单（硬编码，避免依赖日志重解析）
SITES = [
    ('cgpu/cgb258_serialize.rs', [995]),
    ('cgpu/cgb259_snapshot.rs', [580]),
    ('cgpu/cgb260_recover.rs', [514]),
    ('cgpu/cgb261_replay.rs', [515]),
    ('cgpu/cgb262_diffaudit.rs', [473, 517]),
    ('cgpu/cgb263_crashlink.rs', [384, 422]),
    ('cgpu/cgb264_perfudget.rs', [314, 327, 338]),
    ('cgpu/cgb265_telelink.rs', [202, 219]),
    ('cgpu/cgb266_security.rs', [330]),
    ('cgpu/cgb269_apifreeze.rs', [182]),
    ('cgpu/cgb273_telearch.rs', [436]),
    ('cgpu/cgb275_tsbuffer.rs', [449, 461, 478]),
    ('cgpu/cgb276_tasktele.rs', [354, 370, 380]),
    ('cgpu/cgb277_frametele.rs', [328, 344]),
    ('cgpu/cgb278_health.rs', [277, 321, 329]),
    ('cgpu/cgb280_queryapi.rs', [384, 402, 438, 445]),
]


def insert_leak(lines_1b, lineno):
    """在第 lineno 行(1-based)的 alloc::format! 闭括号后插 .leak()。返回新行列表。"""
    idx = lineno - 1
    # format! 起点：可能在起点行，多行 span 时向后续行找闭括号
    text = '\n'.join(lines_1b)
    # 找第 idx 行内 alloc::format!( 的字符偏移
    off = sum(len(x) + 1 for x in lines_1b[:idx])
    line = lines_1b[idx]
    pos = line.find('alloc::format!(')
    assert pos >= 0, 'L%d 无 format!: %r' % (lineno, line[:120])
    d = off + pos + len('alloc::format!(')
    depth = 1
    k = d
    n = len(text)
    in_str = False
    while k < n and depth > 0:
        c = text[k]
        if in_str:
            if c == '\\':
                k += 2
                continue
            if c == '"':
                in_str = False
        elif c == '"':
            in_str = True
        elif c == '(':
            depth += 1
        elif c == ')':
            depth -= 1
            if depth == 0:
                break
        k += 1
    assert depth == 0, 'L%d 括号配对失败' % lineno
    text = text[:k + 1] + '.leak()' + text[k + 1:]
    return text.split('\n')


for rel, linenos in SITES:
    p = os.path.join(SRC, rel)
    raw = open(p, encoding='utf-8', newline='').read()
    crlf = '\r\n' in raw
    s = raw.replace('\r\n', '\n')
    before = s.count('.leak()')
    s = s.replace('.leak()', '')
    removed = before - s.count('.leak()')
    lines_1b = s.split('\n')
    assert removed == len(linenos), '%s: 应清 %d 处 .leak()，实清 %d' % (rel, len(linenos), removed)
    for lineno in sorted(linenos, reverse=True):  # 降行序：早插不影响前行偏移
        lines_1b = insert_leak(lines_1b, lineno)
    s2 = '\n'.join(lines_1b)
    assert s2.count('.leak()') == len(linenos), rel
    if crlf:
        s2 = s2.replace('\n', '\r\n')
    open(p, 'w', encoding='utf-8', newline='').write(s2)
    print('OK', rel, '重插', len(linenos), '处')
print('修复完成')
