#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""从 cargo check 日志提取错误清单（错误码 + 位置 + 摘要），去重输出。"""
import re
import sys

LOG = sys.argv[1] if len(sys.argv) > 1 else '_attic/check3.log'
log = open(LOG, encoding='utf-8', errors='replace').read()
lines = log.split('\n')

out = []
i = 0
while i < len(lines):
    m = re.match(r'^error(?:\[(E\d+)\])?: (.+)', lines[i])
    if m:
        code = m.group(1) or 'PARSE'
        loc = ''
        for j in range(i + 1, min(i + 8, len(lines))):
            lm = re.search(r'--> (varix[\\/].+?):(\d+):(\d+)', lines[j])
            if lm:
                path = lm.group(1).replace('\\', '/')
                path = path.replace('varix/src/', '')
                loc = path + ':' + lm.group(3)
                break
        out.append('%s %s | %s' % (code, loc, m.group(2)[:90]))
    i += 1

seen = set()
for o in out:
    if o in seen:
        continue
    seen.add(o)
    print(o)
print('TOTAL:', len(seen))
