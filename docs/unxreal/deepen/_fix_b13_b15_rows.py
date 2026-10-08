# -*- coding: utf-8 -*-
"""A4 域收官侧修：B13/B15 深化册 8 条残留骨架版判据行 → 深化版（补三段分解段）。

背景：早期会话深化时这 8 条判据行未换为深化版格式（纯功能行数：N｜ 缺
 行（名1 a + 名2 d + 名3 c；测试段不计） 段），致全域严格判据 792/800、
B13 深化累计 5480/6000、B15 4360/6000。差值恰等于 8 条 N 值之和，
补齐后每批深化累计恰回 6000、全域 800 条四段结构闭合。
行级定位：UNX-F####-J 唯一锚 + 骨架版段 纯功能行数：N｜ 行内唯一。
"""
import re

FIX = {
    'deepen/A4-B13.md': [
        (2653, 260, '标记选位 90 + 生命周期 90 + 对账输出 80'),
        (2655, 260, '判定链接力 90 + 执行面断言 90 + 回验闭合 80'),
    ],
    'deepen/A4-B15.md': [
        (2692, 300, '九格执行 110 + 回收不抖 100 + 命中率账 90'),
        (2694, 260, '码族总册 90 + 逐码登记 90 + 交接断言 80'),
        (2695, 280, '并发执行 100 + 账实恒等 100 + 互斥声明 80'),
        (2696, 280, '基线冻结 100 + 触发投递 100 + 时延账 80'),
        (2697, 260, '演练制度 90 + 判据固化 90 + 承载闭合 80'),
        (2698, 260, '数值复核 90 + 映射终核 90 + 差异对账 80'),
    ],
}

for path, items in FIX.items():
    lines = open(path, encoding='utf-8').read().splitlines(True)
    for fid, n, segs in items:
        nums = [int(x) for x in re.findall(r'\d+', segs)]
        assert len(nums) == 3 and sum(nums) == n, (fid, segs, sum(nums))
        old = '纯功能行数：%d｜' % n
        new = '纯功能行数：%d 行（%s；测试段不计）｜' % (n, segs)
        hits = 0
        for i, ln in enumerate(lines):
            # 判据行专属锚：骨架版段 纯功能行数：N｜ 与判据号同行
            # （正文行虽引用 UNX-F####-J1 但不含骨架版段；深化版为 N 行（…） 不含 old）
            if old in ln and 'UNX-F%d-J' % fid in ln:
                assert ln.count(old) == 1, (fid, '骨架版段不唯一', ln.count(old))
                lines[i] = ln.replace(old, new, 1)
                hits += 1
            elif new in ln and 'UNX-F%d-J' % fid in ln:
                hits += 1  # 幂等：已修复过
        assert hits == 1, (fid, '命中行数', hits)
    open(path, 'w', encoding='utf-8', newline='').writelines(lines)
    print(path, '修复', len(items), '条')

# 复验：两册严格判据 20/20 且累计 6000 且三段分解之和守恒
strict = re.compile(r'纯功能行数：(\d+) 行（(.+?)；测试段不计）')
seg_num = re.compile(r'(\d+)\s*$')
for path in FIX:
    t = open(path, encoding='utf-8').read()
    ms = list(strict.finditer(t))
    tot = sum(int(m.group(1)) for m in ms)
    seg_ok = all(
        sum(int(seg_num.search(p.strip()).group(1)) for p in m.group(2).split(' + ')) == int(m.group(1))
        for m in ms)
    print(path, '严格判据 %d 条 累计 %d 分解守恒 %s' % (len(ms), tot, seg_ok))
    assert len(ms) == 20 and tot == 6000 and seg_ok
print('=== B13/B15 判据行修复完成，复验全绿 ===')
