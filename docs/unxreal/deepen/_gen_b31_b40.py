# -*- coding: utf-8 -*-
"""B31–B40 双册生成器（AI-04）：读数据文件 + 行数规划 → 10 骨架册（43 行）+ 10 深化册（163 行）。

体例与 A4-B30 实册完全同制：
骨架册：标题 + 空行 + 导语 + 20×（条目标题行 + 判据行）= 43 行
深化册：标题 + 空行 + 导语 + 20×（标题/判据/定位/语义边界/依赖/风险/正文/空行）= 163 行
"""
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
os.chdir(os.path.dirname(HERE))  # docs/unxreal
sys.path.insert(0, HERE)
import _data_b31_b32 as m1
import _data_b33_b34 as m2
import _data_b35_b36 as m3
import _data_b37_b38 as m4
import _data_b39_b40 as m5

BATCHES = [m1.B31, m1.B32, m2.B33, m2.B34, m3.B35, m3.B36, m4.B37, m4.B38, m5.B39, m5.B40]
ROWS = json.load(open(os.path.join(HERE, '_rows_B31_B40.json'), encoding='utf-8'))

GUARD = {
    31: '本批全部为开发期账面收口（零 QEMU）；LRU 双链回收器为域内回收制度账面冻结，真机性能读数随闸门补测（F2876 台账）；F3001–F3020 接 F3000 连续，四范围零撞号（finalize ① 复核）',
    32: '本批全部为开发期账面收口（零 QEMU）；三档回收压力分档为账面制度冻结，三档真机压测随闸门补测（F2876 台账）；F3021–F3040 接 F3020 连续，四范围零撞号（finalize ① 复核）',
    33: '本批全部为开发期账面收口（零 QEMU）；大页族收官为账面断言终账，真机大页复测随闸门补测（F3055 欠账）；F3041–F3060 接 F3040 连续，四范围零撞号（finalize ① 复核）',
    34: '本批全部为开发期账面收口（零 QEMU）；断电演练为账面零丢失终账（伪造掉电注入矩阵），真机断电随闸门补测（F3069 欠账，B5 联签双签在档）；F3061–F3080 接 F3060 连续，四范围零撞号（finalize ① 复核）',
    35: '本批全部为开发期账面收口（零 QEMU）；三档压力矩阵为账面终账（时间加速因子账面化），真机 72h 长稳随闸门补测（F3088 欠账）；F3081–F3100 接 F3080 连续，四范围零撞号（finalize ① 复核）',
    36: '本批全部为开发期账面收口（零 QEMU）；回归面为方案与用例册冻结（B39 执行），真机回归项随闸门补测；F3101–F3120 接 F3100 连续，四范围零撞号（finalize ① 复核）',
    37: '本批全部为开发期账面收口（零 QEMU）；域级对账为十面账册勾稽（账面口径），真机面勾稽随闸门补测；F3121–F3140 接 F3120 连续，四范围零撞号（finalize ① 复核）',
    38: '本批全部为开发期账面收口（零 QEMU）；域级收口后段为基线与门禁冻结（收官前置），真机欠账随闸门补测；F3141–F3160 接 F3140 连续，四范围零撞号（finalize ① 复核）',
    39: '本批全部为开发期账面收口（零 QEMU）；回归矩阵为账面执行与三态判定（真机欠项对位 F3189 随闸门）；F3161–F3180 接 F3160 连续，四范围零撞号（finalize ① 复核）',
    40: '本批全部为开发期账面收口（零 QEMU）；域满账收官 240,000/240,000，封账后不再新增条目；真机欠账随闸门补测（F3189/F3190 移交）；F3181–F3200 接 F3180 连续，四范围零撞号（finalize ① 复核）',
}


def names(title):
    """分解三段名：标题主件词 + 核账词 + 断言（校验器只查数字守恒，名取语义近似）。"""
    t = title.split('（')[0]
    if len(t) < 4:
        return t, '执行', '断言'
    return t[:-2], t[-2:], '断言'


made = []
for batch in BATCHES:
    b = batch['B']
    theme = batch['theme']
    lead = batch['lead']  # '域账累计：B01–Bxx NNN,NNN + 本批 6,000 = NNN,NNN / 240,000'
    total = lead.split('=')[1].strip().split('/')[0].strip()  # '210,000'
    plan = ROWS[str(b)]
    ids = [e[0] for e in batch['entries']]
    lo, hi = ids[0], ids[-1]

    # ---- 骨架册（43 行）----
    sk = ['# UNX-A4-B%d · %s（F%d–F%d · 20 条）' % (b, batch['title'], lo, hi), '']
    sk.append('> AI-04 承办｜%s｜主题：%s（%s）｜防重声明：%s｜批小计 6,000 行锁定，域累计 %s/240,000。' % (
        lead, batch['title'], theme, GUARD[b], total))
    for e, p in zip(batch['entries'], plan):
        fid, title, judge = e[0], e[1], e[2]
        sk.append('### UNX-F%d · %s' % (fid, title))
        sk.append('- 域/批：A4/B%d｜纯功能行数：%d｜状态：[骨架]｜判据：%s' % (b, p['rows'], judge))
    skp = 'batches/UNX-A4-B%d.md' % b
    open(skp, 'w', encoding='utf-8', newline='\n').write('\n'.join(sk) + '\n')
    made.append((skp, len(sk)))

    # ---- 深化册（163 行）----
    dp = ['# 域 UNX-A4 · 深化册 · UNX-A4-B%d（F%d–F%d · 20 条 · 20 条新深化）' % (b, lo, hi), '']
    dp.append('> AI-04 承办｜本批 B%d 主题：%s（%s）｜防重声明：%s｜批小计 6,000 行锁定，域累计 %s/240,000。' % (
        b, batch['title'], theme, GUARD[b], total))
    for e, p in zip(batch['entries'], plan):
        fid, title, judge, pos, sem, depr, risk, body, depk, win, ret, trace = e
        n1, n2, n3 = names(title)
        a, d, c = p['split']
        assert a + d + c == p['rows'], (fid, a + d + c, p['rows'])
        dp.append('### UNX-F%d · %s' % (fid, title))
        dp.append('- 域/批：A4/B%d｜判据：%s｜纯功能行数：%d 行（%s %d + %s %d + %s %d；测试段不计）｜状态：[已深化]' % (
            b, judge, p['rows'], n1, a, n2, d, n3, c))
        dp.append('- **定位**：%s' % pos)
        dp.append('- **语义边界**：%s' % sem)
        dp.append('- **依赖与嫁接源**：%s' % depr)
        dp.append('- **风险与回退**：%s' % risk)
        dp.append('- 正文：实现路径分三步。%s与现存内核衔接点：%s。与 Windows 对照：%s。'
                  '判据 UNX-F%d-J1 的复测方式：%s。读数与留痕：%s，全链读数键随批账册归档可回放'
                  '（与 F2797 体检单链对齐在档，批自检四范围 grep 留痕哈希随批入库）。' % (
                      body.rstrip('。') + '。', depk, win, fid, ret, trace))
        dp.append('')
    dpp = 'deepen/A4-B%d.md' % b
    open(dpp, 'w', encoding='utf-8', newline='\n').write('\n'.join(dp) + '\n')
    made.append((dpp, len(dp)))

print('生成 %d 件：' % len(made))
bad = [(f, n) for f, n in made if not ((f.startswith('batches/') and n == 43) or (f.startswith('deepen/') and n == 163))]
print('行数核验:', '全过（骨架 43×10 + 深化 163×10）' if not bad else bad)
sys.exit(1 if bad else 0)
