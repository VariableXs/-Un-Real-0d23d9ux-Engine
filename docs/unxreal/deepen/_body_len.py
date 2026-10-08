# -*- coding: utf-8 -*-
"""正文长度检查器：import 目标批脚本（monkey-patch build_book 阻止落盘），列出 20 条正文长度。"""
import sys
import types

mod_name = sys.argv[1]          # 如 _gen_a2_b32
sys.path.insert(0, 'docs/unxreal/deepen')

fake = types.ModuleType('_a2_book_lib')
captured = {}


def fake_build_book(batch, f_start, theme, graft, fangzhong, upstream, entries, rows_total, weiby):
    captured['args'] = (batch, f_start, rows_total)
    captured['entries'] = entries
    return 0, 0


fake.build_book = fake_build_book
sys.modules['_a2_book_lib'] = fake

__import__(mod_name)
batch, f_start, rows_total = captured['args']
entries = captured['entries']
bad = 0
for i, e in enumerate(entries, 1):
    zw = e[7].strip()
    n = len(zw) - 1
    fid = f_start + i - 1
    flag = '' if n >= 300 else '  <-- 不足'
    if n < 300:
        bad += 1
    print('F%d(%2d) %4d%s' % (fid, i, n, flag))
print('批 %d rows_total=%d 不足条数=%d' % (batch, rows_total, bad))
sys.exit(1 if bad else 0)
