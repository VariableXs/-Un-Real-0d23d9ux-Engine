p_master=r"docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md"
p_add=r"docs/Varix/CoRun Varix STAR II · Unxreal/AI-76 · P1 · 尾段增补册（B16–B40 · 500项新功能 · F60301–F60800）.md"
add=open(p_add,encoding="utf-8").read()
sec="\n\n---\n\n# 增补卷 · AI-76 · 波 28 尾产段 UNX-P1 收官（B16–B40 · F60301–F60800 · 500 项新功能 · 2026-10-01 一次对话全部写完明令）\n\n> **增补卷登记（AI-76 · 2026-10-01）**：卷末续追 UNX-P1 尾段 B16–B40 共 500 项新功能（25 批 × 20 条 × 6,000 行 = 150,000 行），与首产段增补卷（300 项）合计全段 40 批 800 条 / 240,000 行域账 100% 落账。批型：B16–B26 E 型错误路径批（11 批）、B27–B36 I 型四方联签批（10 批）、B37–B40 C 型收官批（4 批）。四断言机检（ID 连续零跳号/40 批批守恒 240,000 行/800 主题零重复/判据 800 唯一）ALL PASS。他域账零触碰。\n\n以下为 500 条全量表体（与独立尾段增补册逐字一致）：\n\n"
with open(p_master,"a",encoding="utf-8",newline="\n") as f: f.write(sec+add)
print("appended")
