#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""UNX-D3 骨架索引账生成器（波09-M34 · B31–B40 收官轮）

自深化册 deepen/D3-B31..B40.md 判据单源派生骨架索引账 batches/UNX-D3-B31..B40.md，
判据文本零改写（与深化校验器 drift 判据逐字一致），状态 [已深化]。
B01–B30 骨架账已在前轮收口，本器只产出 B31–B40，不触碰既有批册。

用法：python scripts/unxreal_d3_skeleton_gen.py
"""
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEEPEN = ROOT / 'docs' / 'unxreal' / 'deepen'
BATCH = ROOT / 'docs' / 'unxreal' / 'batches'

THEMES = {
    31: ('D3×D1 同步族对象真联签（一）· 冻结接口对接', '波08-M28', 186_000,
         'B31（D3×D1 属性面真联签）产物+上游 D1 域满账冻结接口（OBJECT_ATTRIBUTES 冻结协议 v1）'),
    32: ('D3×D2 进程继承链真联签（二）· 冻结接口对接', '波09-M34', 192_000,
         'B31 产物+上游 D2 域进程对象冻结接口+Windows 句柄继承公开语义'),
    33: ('安全描述符与安全质量面真联签（三）', '波09-M34', 198_000,
         'B31–B32 产物+SECURITY_DESCRIPTOR 公开语义+Windows ACL 行为观测'),
    34: ('D3×D4/D5/D6 文件型与网络命名对象真联签（四）', '波09-M34', 204_000,
         'B31–B33 产物+对端 D4/D5/D6 冻结接口+Windows 名字空间行为观测'),
    35: ('I 段真联签总聚合（五）· B31–B34 总归并', '波09-M29', 210_000,
         'B31–B34 四批真联签产物+B30 预演聚合体例+对端冻结接口'),
    36: ('C 型收官（一）· 全域判据总回归与 ktest 总批', '波09-M34', 216_000,
         'B01–B35 全量判据+F14199 总批体例+先行域收官判例'),
    37: ('C 型收官（二）· 句柄泄漏总账与资源红线核账', '波09-M34', 222_000,
         '全域句柄族判据+F14229 配平体例+F14191 五轴口径+硬件红线制度'),
    38: ('C 型收官（三）· 坏档矩阵总回归与崩溃一致性终验', '波09-M34', 228_000,
         'E 段十批坏档矩阵+B12 崩溃一致性+F13990 恢复终口径'),
    39: ('C 型收官（四）· 性能核账与体验完整性终账', '波09-M34', 234_000,
         '全域性能口径族（F14217/F14235）+体验十四章纪律+20 维验收制度'),
    40: ('C 型收官（五）· 域收官宣告与满账终对账', '波09-M34', 240_000,
         'B01–B39 全域产物+先行域收官判例（F1600/F3200/F7199）+任务书验收判据六条'),
}

META_RE = re.compile(
    r'### (UNX-F\d+) · (.+)\n- 域/批：D3/B(\d+)｜判据：(UNX-F\d+-J1 .*?)｜纯功能行数：(\d+) 行（.*?）｜状态：\[已深化\]')


def main():
    total_problems = []
    for b in range(31, 41):
        src = DEEPEN / ('D3-B%02d.md' % b)
        txt = src.read_text(encoding='utf-8')
        entries = META_RE.findall(txt)
        if len(entries) != 20:
            total_problems.append('D3-B%02d: 解析条目 %d != 20' % (b, len(entries)))
            continue
        fid0, fid1 = entries[0][0], entries[-1][0]
        theme, wave, cum, graft = THEMES[b]
        rows_sum = sum(int(e[4]) for e in entries)
        if rows_sum != 6000:
            total_problems.append('D3-B%02d: 行数和 %d != 6000' % (b, rows_sum))
        lines = []
        lines.append('# UNX-D3-B%02d · %s（%s–%s · 20 条）' % (b, theme, fid0, fid1))
        lines.append('')
        lines.append('> AI-18 承办｜域账累计：本批 6,000 / %d｜波次：%s（D3 域收官轮）｜嫁接源：%s｜防重：与相邻批按治理层级/对端域分册，判据颗粒不重复（详见各深化册批头防重声明）｜派生声明：本骨架账自深化册 docs/unxreal/deepen/D3-B%02d.md 判据单源派生（判据文本零改写，生成器 scripts/unxreal_d3_skeleton_gen.py），深化已收口故条目状态标 [已深化]（与总纲 §7.3-D3 台账同步）' % (cum, wave, graft, b))
        lines.append('')
        for fid, title, bb, crit, r in entries:
            if int(bb) != b:
                total_problems.append('D3-B%02d: %s 批号不符' % (b, fid))
            lines.append('### %s · %s' % (fid, title))
            lines.append('- 域/批：D3/B%02d｜纯功能行数：%s｜状态：[已深化]｜判据：%s' % (b, r, crit))
            lines.append('')
        out = BATCH / ('UNX-D3-B%02d.md' % b)
        out.write_text('\n'.join(lines).rstrip() + '\n', encoding='utf-8')
        print('written %s (%d entries, rows=%d)' % (out.name, len(entries), rows_sum))
    if total_problems:
        print('PROBLEMS:')
        for p in total_problems:
            print('  -', p)
        raise SystemExit(1)
    print('ALL OK')


if __name__ == '__main__':
    main()
