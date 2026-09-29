# -*- coding: utf-8 -*-
# handoff.json A4 块回填：finalized 15→30 / rows 90,000→180,000 / entries 300→600 / deepen_books +15 / sync_notes / 顶部标量
import json

P = 'handoff.json'
h = json.load(open(P, encoding='utf-8'))

wc = {16: 19250, 17: 17701, 18: 16290, 19: 16994, 20: 16571,
      21: 15664, 22: 15599, 23: 15685, 24: 15395, 25: 15575,
      26: 15407, 27: 16013, 28: 15774, 29: 17049, 30: 17183}

a4 = h['domain_ledger_progress']['A4']

# 1) finalized_list +15
new_list = [f'UNX-A4-B{b:02d}' for b in range(16, 31)]
assert all(x not in a4['finalized_list'] for x in new_list)
a4['finalized_list'] += new_list
a4['finalized_batches'] = 30
a4['skeleton_batches'] = 0
a4['rows_locked'] = 180000
a4['rows_deepened_locked'] = 180000
a4['entries_skeleton'] = 0
a4['entries_deepened'] = 600

# 2) entries_deepened_note 追加第二轮
note2 = ('；第二轮 B16–B30 续领深化收口 300 条（F2701–F3000）'
 '：B16 参数册基座与压测方案冻结 6,000 行（19,250 字，F2719 参数册基座+F2718 槽位账前向契约立约）'
 '+ B17 三册核对链与登记册扩页 6,000 行（17,701 字，F2739 核对链第 1 处立约）'
 '+ B18 参数扩展页与对齐断言 6,000 行（16,290 字，F2750/F2762 扩展页）'
 '+ B19 压测执行段前段 6,000 行（16,994 字）'
 '+ B20 压测执行段收口 6,000 行（16,571 字，F2782 兑现 F2718 契约兑现链闭合）'
 '+ B21 回收压测段 6,000 行（15,664 字）'
 '+ B22 双轨产线登记段 6,000 行（15,599 字，F2832 行数偏差 320→300 收敛归零留痕）'
 '+ B23 COW 联动压测段 6,000 行（15,685 字，F2844 补字留痕）'
 '+ B24 真机欠账登记段 6,000 行（15,395 字，8 项欠账挂 F2876 双轨台账）'
 '+ B25 调度联动压测段 6,000 行（15,575 字）'
 '+ B26 页缓存压测十面 6,000 行（15,407 字，F2906/F2912 补字收口）'
 '+ B27 COW-页缓存联合压测与三判例联合复核 6,000 行（16,013 字）'
 '+ B28 域级压力矩阵（16GB 三档 72h 零 OOM 误杀开发期账面预演）6,000 行（15,774 字，F2945/F2958 真机欠账登记）'
 '+ B29 域级回归与契约兑现 6,000 行（17,049 字，F2964 终核双签兑现 F2729、F2965 全条目判据复测 560 条零抽样、F2977 四源欠账归集、F2979 域级对账预演 168,000 行守恒预核）'
 '+ B30 扩产段域级收口与总对账 6,000 行（17,183 字，F2982 满账口径冻结+B31–B40 预算核对、F2984 行数守恒终账 90,000、F2992 域冻结接口五件终账、F2997 四门联合判定、F2998 收官声明与 B31–B40 移交预告）'
 '；契约兑现链 F2718→F2782、F2729→F2964 闭合；三册核对链 13 处（F2739→F2973）逐处衔接；自检总闸 F2797 体检单链至第十五件（F2972 累积/F2994 登记）'
 '；finalize 断言链机械面全绿：①防重四范围 grep——F2701–F3000 标题行注册全域唯一、域外零命中 ②300 条 ID 唯一连续 ③行数守恒 15×6,000=90,000（域累计 180,000/240,000） ④判据行四段结构+复测锚+判据编号一致性 300/300 ⑤300 条正文 ≥300 字整行实计全域清零（前会话遗留 188 条超短正文经本会话扩写归零，校验器 deepen/_a4_check.py 全绿留痕）；深化册全文 wc -m 合计 246,150 字')
a4['entries_deepened_note'] += note2
a4['open_bugs'] = 0

# 3) deepen_books +15
for b in range(16, 31):
    e = f'docs/unxreal/deepen/A4-B{b}.md（20 条深化，全文 {wc[b]:,} 字，6,000 行锁定零偏离）'
    assert e not in h['deepen_books']
    h['deepen_books'].append(e)

# 4) sync_notes 追加
h['sync_notes'].append(
 'AI-04 波08-M19 提交范围仅限 AI-04 自有产物：handoff.json 本文件（domain_ledger_progress.A4 块 finalized 15→30/rows_locked 90,000→180,000/entries_deepened 300→600/entries_deepened_note 第二轮追加、deepen_books +15、sync_notes 本条；基于磁盘现版增量滚动，他会话记录全部保留）、总纲 §7.3-A4 B16–B30 十五行 [骨架]→[已深化]+字数回填+域小结/修订记录更新、根 MD §二/§三/§四/§六 四处、batches/UNX-A4-B16..B30.md 15 件+deepen/A4-B16..B30.md 15 件+deepen/_a4_check.py 校验器+扩写脚本链。他会话产物均不纳入，不越权代提交。')

# 5) 顶部标量链式更新
h['updated_at'] = ('波08-M19（AI-04 A4 域 B16–B30 续领深化收口 300 条/90,000 行——域累计 600 条/180,000/240,000；finalize 五步断言链 15 批全过、'
 '前会话遗留 188 条超短正文扩写归零、B22 F2832 行数偏差收敛留痕、契约兑现链 F2718→F2782/F2729→F2964 闭合；'
 'B31–B40 待领 200 条账余 60,000 行；前一版本为 波08-M17（AI-15 C5 风险台账维护）；再前链（A2/A4/A5/B4/D1 等各域收口与维护轮）逐版保留在案）')
h['last_session'] = ('AI-04 收口轮（波08-M19：A4 域 B16–B30 续领深化收口 300 条——B16–B20 参数册与三接口冻结兑现段/B21–B25 回收·COW·调度联动压测段/'
 'B26–B28 页缓存与域级压力矩阵段/B29–B30 域级回归与总对账收口段；自建 deepen/_a4_check.py 六项断言全绿；台账三处回填完成）')

json.dump(h, open(P, 'w', encoding='utf-8'), ensure_ascii=False, indent=1)
print('handoff.json 回填完成')
h2 = json.load(open(P, encoding='utf-8'))
a = h2['domain_ledger_progress']['A4']
print('复核: finalized', a['finalized_batches'], '/ rows', a['rows_locked'], '/ entries', a['entries_deepened'], '/ deepen_books A4 条数', sum(1 for e in h2['deepen_books'] if 'A4-B' in e))
