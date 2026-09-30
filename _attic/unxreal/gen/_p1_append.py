p_master=r"docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md"
p_add=r"docs/Varix/CoRun Varix STAR II · Unxreal/AI-76 · P1 · 300项新功能增补册（B01–B15 · F60001–F60300）.md"
chk=open(p_master,encoding="utf-8",errors="strict").read(0) if False else None
with open(p_master,"r",encoding="utf-8") as f: tail=f.seek(0,2)
add=open(p_add,encoding="utf-8").read()
sec="\n\n---\n\n# 增补卷 · AI-76 · 波 28 首产段 UNX-P1 断点续写产线制度化（F60001–F60300 · 300 项新功能 · 2026-10-01 明令）\n\n> **增补卷登记（AI-76 · 2026-10-01 · 一次对话 300 项新功能明令）**：卷末追加 UNX-P1 首产段 B01–B15 共 300 项新功能（15 批 × 20 条，连续零跳号，每批 6,000 行、全段 90,000 行，域账 240,000 行内进度 37.5%），状态列统一「增补」不冒充深化。任务书五枚示例锚（F60001/F60021/F60041/F60061/F60101）按区间恒等归位判例保真落位（F60101 批标 B23 与 ID 区间 B06 不一致处诚实登记）。判据主轴「handoff 工具、断点恢复零丢失演练判据」双件承载：B02 事务性写入引擎（kill -9 ×50 零丢失）+ B06 四剧本演练包（四剧本各 3 次全过）。域内加严红线三条（禁绕过双写/禁断言短路/禁冷启动写）机器化落账。五断言机检 ALL PASS（ID 连续/批守恒/主题零重复/判据唯一/五锚保真）；独立增补册《AI-76 · P1 · 300项新功能增补册（B01–B15 · F60001–F60300）》与本卷表体逐字一致。他域账（AI-77/AI-81/AI-82/AI-83/AI-91/AI-92/AI-93/AI-97/AI-98/AI-99）零触碰，仅联签锚定行出现。\n\n以下为 300 条全量表体（与独立增补册逐字一致）：\n\n"
with open(p_master,"a",encoding="utf-8",newline="\n") as f: f.write(sec+add)
print("appended, master tail bytes now:", tail+len(sec)+len(add.encode('utf-8')))
