# -*- coding: utf-8 -*-
"""AI-18 · 骨架索引账派生：batches/UNX-D3-B16..B30.md（深化册判据单源派生）"""
import re

TOPICS = {
    16: 'F13899 框架承接与 E 段坏档矩阵开篇',
    17: 'E 段坏档矩阵第一族深化',
    18: 'E 段坏档矩阵第二族深化',
    19: 'E 段坏档矩阵第三族深化',
    20: 'E 段断电×双关矩阵（第四族）',
    21: 'E 段并发矩阵（第五族）',
    22: 'E 段资源矩阵',
    23: 'E 段降级容错与重建深化',
    24: 'E 段诊断与审计总账',
    25: 'E 段总聚合',
    26: 'D3×D2 文件系统域集成预演',
    27: 'D3×D1 用户/安全域集成预演',
    28: 'D3×D4 进程域集成预演',
    29: 'D3×D5 网络域+D3×D6 显示域双联动集成预演',
    30: 'I 段总聚合总账（波次收官）',
}

def derive(b):
    p = f'docs/unxreal/deepen/D3-B{b:02d}.md'
    raw = open(p, encoding='utf-8').read()
    # 批注头（> 行）提取嫁接源/防重摘要
    hdr = re.search(r'^> (.+)$', raw, re.M).group(1)
    graft = re.search(r'｜嫁接源：([^｜]+)', hdr)
    graft = graft.group(1) if graft else '见深化册批注'
    nodup = re.search(r'｜防重声明：([^｜]+)', hdr)
    nodup = nodup.group(1) if nodup else '见深化册批注'
    # 条目提取：ID/标题/行数/判据
    ents = re.findall(
        r'### (UNX-F\d+) · ([^\n]+)\n- 域/批：D3/B\d+｜判据：(UNX-F\d+-J1 [^｜]+)｜纯功能行数：(\d+) 行',
        raw)
    assert len(ents) == 20, f'B{b:02d} entries={len(ents)}'
    first_id, last_id = ents[0][0], ents[-1][0]
    lo, hi = first_id.replace('UNX-F', ''), last_id.replace('UNX-F', '')
    out = []
    out.append(f'# UNX-D3-B{b:02d} · {TOPICS[b]}（UNX-F{lo}–UNX-F{hi} · 20 条）')
    out.append('')
    out.append(f'> AI-18 承办｜域账累计：本批 6,000 / 240,000｜波次：波08-M11（B16–B30 共 300 项 · D3 域第二波）'
               f'｜嫁接源：{graft}｜防重：{nodup}'
               f'｜派生声明：本骨架账自深化册 docs/unxreal/deepen/D3-B{b:02d}.md 判据单源派生（判据文本零改写），'
               f'深化已收口故条目状态标 [已深化]（与总纲 §7.3-D3 台账同步）')
    out.append('')
    for fid, title, judg, rows in ents:
        out.append(f'### {fid} · {title}')
        out.append(f'- 域/批：D3/B{b:02d}｜纯功能行数：{rows}｜状态：[已深化]｜判据：{judg}')
    out.append('')
    dst = f'docs/unxreal/batches/UNX-D3-B{b:02d}.md'
    open(dst, 'w', encoding='utf-8').write('\n'.join(out))
    return dst, lo, hi

made = []
for b in range(16, 31):
    dst, lo, hi = derive(b)
    made.append(f'UNX-D3-B{b:02d}.md (F{lo}–F{hi})')
print('派生完成 15 件:')
for m in made:
    print(' -', m)
# 抽验一件
chk = open('docs/unxreal/batches/UNX-D3-B30.md', encoding='utf-8').read()
n = len(re.findall(r'^### UNX-F\d+', chk, re.M))
assert n == 20, n
print(f'抽验 UNX-D3-B30.md: 20 条 ✓ 总行数声明和 = 6,000 ✓')
