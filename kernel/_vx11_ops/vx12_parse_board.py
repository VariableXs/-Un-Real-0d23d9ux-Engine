# -*- coding: utf-8 -*-
"""解析 taskboard 卡面信息（只读）。"""
import io
import re

SRC = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\dist\taskboard.md"


def parse():
    cards = {}
    cur = None
    with io.open(SRC, 'r', encoding='utf-8') as f:
        for line in f:
            m = re.match(r'^## \[UNX-F(\d+)\]', line)
            if m:
                cur = int(m.group(1))
                cards[cur] = {'status': '?', 'owner': ''}
                continue
            if cur is None:
                continue
            if line.startswith('- 状态:'):
                状态 = line.split(':', 1)[1].strip()
                cards[cur]['status'] = 状态
    return cards


if __name__ == '__main__':
    c = parse()
    print('cards:', len(c))
    from collections import Counter
    st = Counter(v['status'] for v in c.values())
    print('status:', dict(st))
