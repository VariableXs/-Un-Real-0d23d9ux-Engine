# -*- coding: utf-8 -*-
"""通用正文补长工具：python _body_patch.py <脚本名> (锚点␟插入语)；锚点唯一性 count==1 强校验。
锚点/插入语从 stdin 读入，每行一条，格式：锚点␟插入语（插入语置于锚点之后）。"""
import io
import sys

P = sys.argv[1]
t = io.open(P, encoding='utf-8').read()
done = 0
for line in io.open(0, encoding='utf-8'):
    line = line.rstrip('\n')
    if not line.strip():
        continue
    anchor, ins = line.split('␟', 1)
    n = t.count(anchor)
    assert n == 1, '锚点非唯一(%d): %s' % (n, anchor[:30])
    t = t.replace(anchor, anchor + ins, 1)
    done += 1
io.open(P, 'w', encoding='utf-8', newline='').write(t)
print('%s 片段插入 %d 处' % (P, done))
