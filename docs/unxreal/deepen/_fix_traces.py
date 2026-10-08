# -*- coding: utf-8 -*-
"""修复同文 trace 条目的错段/漏扩（按行定位，行首 (fid,' 唯一）。"""
import sys

E = {
    3020: '四项守恒断言原始输出件与四范围 grep 留痕哈希随批归档；缺号注入检出位次点验单在档，两项新增查逐项勾稽结果件可回放',
    3040: '四项守恒断言原始输出与四范围 grep 留痕哈希随批归档；缺号注入检出位次点验单在档，两项新增查勾稽结果件可回放',
    3059: 'B33 段四项健康读数（清点/终账/欠账/自检）挂链件哈希归档；四类破坏注入检出位次点验单在档，总闸并入注记可回放',
    3060: '四项守恒断言原始输出与四范围 grep 留痕哈希随批归档；缺号注入检出位次点验单在档，两项新增查勾稽结果件可回放',
    3077: 'B34 段四项健康读数（方案/零丢失/联签/欠账）挂链件哈希归档；四类破坏注入检出位次点验单在档，总闸并入注记可回放',
    3080: '四项守恒断言原始输出与四范围 grep 留痕哈希随批归档；缺号注入检出位次点验单在档，两项新增查勾稽结果件可回放',
}
OLD = '自检读数键与 F2797 体检单链对齐可查'
OLD2 = '体检键与 F2797 链面对齐可查'
TARGET = {3020: OLD, 3040: OLD, 3059: OLD2, 3060: OLD, 3077: OLD2, 3080: OLD}
FILES = {3020: '_data_b31_b32.py', 3040: '_data_b31_b32.py',
         3059: '_data_b33_b34.py', 3060: '_data_b33_b34.py',
         3077: '_data_b33_b34.py', 3080: '_data_b33_b34.py'}

for fid, old in TARGET.items():
    f = FILES[fid]
    lines = open(f, encoding='utf-8').read().split('\n')
    hits = [i for i, ln in enumerate(lines) if ln.startswith('(%d,\'' % fid)]
    if len(hits) != 1:
        sys.exit('F%d 行定位失败: %d 处' % (fid, len(hits)))
    i = hits[0]
    ln = lines[i]
    # 行内把 old 起始的整段 trace 重建为 old + '；' + 正确扩写语：
    # 清理错段 = 截掉首个 '；' 后原有内容再接正确扩写语；漏扩 = 直接接。
    pos = ln.find(old)
    if pos < 0:
        sys.exit('F%d 行内锚缺失' % fid)
    tail_pos = ln.find("')", pos)  # trace 字面量闭合于 ')
    lines[i] = ln[:pos] + old + '；' + E[fid] + ln[tail_pos:]
    open(f, 'w', encoding='utf-8', newline='').write('\n'.join(lines))
    print('F%d 已重建 trace（行 %d）' % (fid, i + 1))

# 复验：6 条 trace 恰为 old/old2 + 本条扩写语，且无重复段
import _data_b31_b32 as a
import _data_b33_b34 as b
want = {fid: TARGET[fid] + '；' + E[fid] for fid in TARGET}
got = {e[0]: e[11] for B in [a.B31, a.B32, b.B33, b.B34] for e in B['entries'] if e[0] in want}
bad = [f for f in want if got.get(f) != want[f]]
print('复验:', '全过' if not bad else bad)
sys.exit(1 if bad else 0)
