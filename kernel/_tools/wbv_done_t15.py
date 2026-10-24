# -*- coding: utf-8 -*-
"""流式写回 F4015-F4070（T01-T05，ves255-ves310）56 张卡为已完成。AI-WBV1022"""
import io
import os
import re

BOARD = os.path.join(os.path.dirname(__file__), '..', '..', 'dist', 'taskboard.md')
TMP = BOARD + '.tmp_wbv2'

RANGES = list(range(4015, 4071))  # 4015..4070

# ves 编号映射：ves255..ves310 -> F4015..F4070
def ves_for(f):
    return 255 + (f - 4015)

RESULT = 'ves{0}_*.rs 全绿（cargo check svstar2 域零错误）；判据 run_ves{0}_checks 已注册 mod.rs；纯功能实现+判据+测试三件齐。'

pat_head = re.compile(r'^## \[(VE-F(\d+))\]')

cur_num = None
cur_match = False
in_file = False
written = 0
with io.open(BOARD, 'r', encoding='utf-8', errors='replace') as fin, \
     io.open(TMP, 'w', encoding='utf-8', newline='') as fout:
    for line in fin:
        m = pat_head.match(line)
        if m:
            cur_num = int(m.group(2))
            cur_match = cur_num in RANGES
            fout.write(line)
            continue
        if cur_match and line.startswith('- 状态:'):
            fout.write('- 状态: 已完成\n')
            written += 1
        elif cur_match and line.startswith('- 结果:'):
            fout.write('- 结果: ' + RESULT.format(ves_for(cur_num)) + '\n')
            written += 1
        elif cur_match and line.startswith('- 时间:'):
            fout.write('- 时间: 2026-10-22 22:45 AI-WBV1022\n')
            written += 1
        else:
            fout.write(line)

os.replace(TMP, BOARD)
print('written fields:', written)
print('size:', os.path.getsize(BOARD))
