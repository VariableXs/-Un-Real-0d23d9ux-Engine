# -*- coding: utf-8 -*-
"""AI-80 · P5 v2 再落件：对当前主册做段内整替（H0→首产段终 边界内替换，不动他域内容），并保证登记行为 v2。"""
import io, os, re, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _p5_firstprod as g

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
M = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")
H0 = '# 增补卷 · AI-80 · UNX-P5 里程碑发布与年度镜像 · 首产段 B01–B15（F63201–F63500 · 300 项）'
END = '**（AI-80 首产段终）**'

V2_REG_KEY = '终审证据树整批归位 B11、四站批顺移 B12'

with io.open(M, encoding='utf-8') as f:
    m = f.read()
cnt = m.count(H0)
assert cnt == 1, f'AI-80 段标题数 {cnt} != 1'
h0 = m.find(H0)
h1 = m.find(END, h0)
assert h1 > h0, '段尾标记未找到'
h1 += len(END)
old_seg = m[h0:h1]

rows = g.build_rows()
new_seg = g.build_master_section(rows).strip()

# 登记行：若还是 v1 文案则升级为 v2
v1_key = '批位差异（任务书 B09–B20 M 型标注 vs 连续零跳号推算 B07–B18）按连续零跳号公理恒等归位并诚实登记（AI-62 判例）'
v2_reg = g.REG_LINE.replace(
    '批位差异（任务书 B09–B20 M 型标注 vs 连续零跳号推算 B07–B18）按连续零跳号公理恒等归位并诚实登记（AI-62 判例）',
    '批位差异（任务书 B09–B20 M 型标注 vs 连续零跳号推算 B07–B18；F63401 任务书批标 B12 vs 推算 B11——终审证据树整批归位 B11、四站批顺移 B12）按连续零跳号公理恒等归位并诚实登记（AI-62/AI-74 判例）',
).replace(
    '生成器 docs/unxreal/gen/_p5_firstprod.py 五断言 ALL PASS exit=0。',
    '生成器 docs/unxreal/gen/_p5_firstprod.py（数据）+ _p5_repair.py（主册段整替修复 v2）五断言 ALL PASS exit=0。',
)
if v1_key in m:
    m = m.replace(g.REG_LINE, v2_reg, 1)
    print('登记行 v1→v2 已升级')
elif V2_REG_KEY in m:
    print('登记行已是 v2')
else:
    print('警告：未找到 v1/v2 登记行文案（可能被并行窗口改写），登记行未动')

m2 = m.replace(old_seg, new_seg, 1)
assert m2.count(H0) == 1
assert 'UNX-F63401 | 终审证据树组装器（R1–R10） | 480' in m2
assert m2.count('UNX-F63401 |') == 1, 'F63401 行不唯一'
# 他域内容零丢失断言：除替换段与登记行外，其余字节完全一致
pre_old, post_old = m[:h0], m[h1:]
i2 = m2.find(H0)
assert m2[:i2] == pre_old.replace(g.REG_LINE, v2_reg, 1) or m2[:i2] == pre_old or v1_key in m, '段前内容被意外改动'
assert m2[i2 + len(new_seg):] == post_old, '段后内容被意外改动'
with io.open(M, 'w', encoding='utf-8', newline='\n') as f:
    f.write(m2)
print('段内整替完成：v2 已落主册（他域字节零改动断言过）')
