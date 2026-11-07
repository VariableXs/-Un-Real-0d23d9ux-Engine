# -*- coding: utf-8 -*-
"""导出卡面 81000..81300 到文本（只读）。"""
import io
import re

SRC = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\dist\taskboard.md"
OUT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\kernel\_vx11_ops\vx12_cards_81xxx.txt"

lo, hi = 81000, 81300
cur = None
buf = []
blocks = {}

with io.open(SRC, 'r', encoding='utf-8') as f:
    for line in f:
        m = re.match(r'^## \[UNX-F(\d+)\]', line)
        if m:
            if cur in blocks:
                blocks[cur].append('===END===')
            cur = int(m.group(1))
            blocks[cur] = [line]
            continue
        if cur in blocks:
            blocks[cur].append(line)

with io.open(OUT, 'w', encoding='utf-8') as w:
    for n in range(lo, hi + 1):
        b = blocks.get(n)
        if b:
            w.write(''.join(b))
            w.write('\n\n')
print('dumped')
