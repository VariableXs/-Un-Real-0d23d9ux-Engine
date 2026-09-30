# -*- coding: utf-8 -*-
"""B4 收官轮写锁：总纲 §7.3-B4 B31–B40 十行 [未动]→[骨架] + 域小结声明 + 修订记录追加。"""
import io, sys

P = r'docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal · 总纲与施工书.md'
t = io.open(P, encoding='utf-8').read()

def rep1(old, new):
    global t
    assert t.count(old) == 1, ('rep1 not unique: %r (count=%d)' % (old[:80], t.count(old)))
    t = t.replace(old, new)

# 1) 十行翻转 [未动]→[骨架]，承接 AI-09
rows = [
    (31, 'F7001–F7020'), (32, 'F7021–F7040'), (33, 'F7041–F7060'), (34, 'F7061–F7080'),
    (35, 'F7081–F7100'), (36, 'F7101–F7120'), (37, 'F7121–F7140'), (38, 'F7141–F7160'),
    (39, 'F7161–F7180'), (40, 'F7181–F7200'),
]
for idx, rng in rows:
    rep1('| UNX-B4-B%02d | %s | 20 | [未动] | 0 | 待领 |' % (idx, rng),
         '| UNX-B4-B%02d | %s | 20 | [骨架] | 0 | AI-09 |' % (idx, rng))

# 2) 域小结 B31–B40 声明
rep1('· B31–B40 待领（预算余 60,000 行）。上游 AI-02 MSI-X 分配接口',
     '· B31–B40 收官轮认领开工（AI-09 第三轮，Variable 明令本会话 200 项新功能域收官：B31–B32 热插拔尾段 40 条——承接 F6998 交接页三节，消费面联测与 v2 演进/真机补测规程与段收官；B33–B40 压测账与回归收官 160 条——压测总谱/故障注入扩谱/长稳 soak/性能核账/回归矩阵三维冻结/三主轴回归/文档对账与消费域移交联签/域收官总验证与闭账）。上游 AI-02 MSI-X 分配接口')

# 3) 修订记录追加（接在 finalize 二轮之后）
old_tail = '真机判据三项（抽插真机全量/PHY 真实盘/背板联动）随闸门补测登记（F6976/F6997，open_risks R-B4-004）；B31–B32 交接页（F6998）落盘供下轮续领承接。'
assert t.count(old_tail) == 1, 'old_tail'
new_tail = old_tail + ('\n> 修订记录（AI-09 会话·收官轮写锁）：B31–B40 收官轮写锁（§6.2 规则二续领，Variable 明令本会话 200 项新功能域收官）：'
    'B31–B32 热插拔尾段 40 条（消费面联测与事件链 v2 演进预留/真机补测规程与热插拔段收官宣告，承接 F6998 交接页主题续接/接口延续/判据衔接三节）；'
    'B33–B40 压测账与回归收官 160 条（B33 压测账总谱与场景矩阵/B34 故障注入压测面扩谱/B35 长稳 soak 与泄漏哨兵账/B36 性能核账与红线门禁/'
    'B37 回归矩阵三维冻结与执行序/B38 三主轴回归执行与旧用例清账/B39 文档对账与消费域移交联签/B40 域收官总验证与闭账——F7200 域收官标志条+域账封账，域 240,000/240,000 满账）；'
    'B31–B40 十行 [未动]→[骨架]，承接会话 AI-09；域小结同步"B31–B40 认领开工"声明。')
t = t.replace(old_tail, new_tail)

io.open(P, 'w', encoding='utf-8', newline='\n').write(t)
print('LOCK OK: 10 rows -> [骨架]/AI-09; 小结+修订记录 appended (%d chars)' % len(t))
