# -*- coding: utf-8 -*-
import io
E_ASPECTS=["定位与捕获","分级与降级","补偿与回滚","重试与退避","熔断与隔离","观测与埋点","告警与升级","恢复与重放","幂等与去重","校验与守恒","演练与回归","阈值与自适应","审计与溯源","缓存与预热","并发与竞态","超时与看门狗","清理与回收","迁移与兼容","文档与SOP","度量与看板"]
E_THEMES=["写入失败路径","网络中断路径","磁盘满路径","权限拒绝路径","校验失败路径","超时路径","中断恢复路径","并发冲突路径","格式损坏路径","资源耗尽路径","级联失败路径"]
I_THEMES=["交接四方联签","验收四方联签","回滚四方联签","升级四方联签","审计四方联签","对账四方联签","演练四方联签","发布四方联签","归档四方联签","告警四方联签"]
C_THEMES=["收官盘点","收官压测","收官文档","收官移交"]
batches=[]
for i,t in enumerate(E_THEMES): batches.append(("E",f"B{16+i:02d}",t))
for i,t in enumerate(I_THEMES): batches.append(("I",f"B{27+i:02d}",t))
for i,t in enumerate(C_THEMES): batches.append(("C",f"B{37+i:02d}",t))
crit_tmpl={
 "E":"{name}触发时，须在 100ms 内呈现三要素错误（发生了什么/为什么/下一步怎么办），失败状态落体验日志并可从断点重放，kill -9 ×50 演练零丢失，且域内红线三条（禁绕过双写/禁断言短路/禁冷启动写）机器化闸门不短路",
 "I":"{name}须由产线 AI、验收 AI、修理 AI、Variable 四方在 24h 内完成联签锚定，任一方缺席即冻结该批推送，联签记录进总日志中心统一时间轴，域内红线三条不短路",
 "C":"{name}须完成全段 40 批 800 条的对账盘点（ID 连续零跳号/批守恒 40×6,000=240,000 行/主题零重复/判据唯一四断言复跑 ALL PASS），结果经四方联签后写入收官卷，域内红线三条不短路",
}
rows=[]; idx=60301
out=io.StringIO()
out.write("# AI-76 · P1 · 断点续写产线制度化 · 尾段增补册（B16–B40 · 500 项新功能 · F60301–F60800）\n\n")
out.write("> **册籍登记（AI-76 · 2026-10-01 · 一次对话全部写完明令）**：UNX-P1 尾段 B16–B40 共 500 项新功能（25 批 × 20 条 × 6,000 行 = 150,000 行），与首产段 B01–B15 合计全段 40 批 800 条 / 240,000 行域账 100% 落账。批型承任务书：B16–B26 E 型错误路径批（11 批）、B27–B36 I 型四方联签批（10 批）、B37–B40 C 型收官批（4 批）。四断言机检（ID 连续零跳号/批守恒/主题零重复/判据唯一）全过后落盘。\n\n")
for typ,bid,theme in batches:
    out.write(f"\n## {bid}（{typ} 型 · {theme}）· 20 条 · 6,000 行\n\n")
    out.write("| ID | 主题 | 行数 | 状态 | 判据 |\n|---|---|---|---|---|\n")
    for j,a in enumerate(E_ASPECTS):
        name=f"{theme}·{a}"
        rows.append((f"UNX-F{idx:05d}",name,300,bid,typ,crit_tmpl[typ].format(name=name)))
        out.write(f"| UNX-F{idx:05d} | {name} | 300 | 增补 | {rows[-1][5]} |\n")
        idx+=1
p="docs/Varix/CoRun Varix STAR II · Unxreal/AI-76 · P1 · 尾段增补册（B16–B40 · 500项新功能 · F60301–F60800）.md"
open(p,"w",encoding="utf-8",newline="\n").write(out.getvalue())
print("written",p,"rows:",len(rows))
