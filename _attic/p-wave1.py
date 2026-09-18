# -*- coding: utf-8 -*-
"""推进总表第1波勾选"""
import io

p = 'docs/VARIX双域系统剩余任务推进总表.md'
s = io.open(p, encoding='utf-8').read()

pairs = [
    ("- [ ] **步骤 1.1**（AI-P）：任务 10", "- [x] **步骤 1.1**（AI-P）：任务 10"),
    ("- [ ] **步骤 1.2**（AI-P）：任务 47", "- [x] **步骤 1.2**（AI-P）：任务 47"),
    ("- [ ] **步骤 1.3**（AI-P）：任务 66", "- [x] **步骤 1.3**（AI-P）：任务 66"),
    ("- [ ] **步骤 1.4**（AI-B）：任务 40", "- [x] **步骤 1.4**（AI-B）：任务 40"),
    ("- [ ] **步骤 1.5**（AI-B）：任务 65", "- [x] **步骤 1.5**（AI-B）：任务 65"),
    ("- [ ] **步骤 1.6**（AI-V）：任务 36", "- [x] **步骤 1.6**（AI-V）：任务 36"),
    ("- [ ] **步骤 1.7**（AI-S）：任务 63", "- [x] **步骤 1.7**（AI-S）：任务 63"),
    ("- [ ] **步骤 1.8**（AI-S）：任务 70", "- [x] **步骤 1.8**（AI-S）：任务 70"),
]
for old, new in pairs:
    assert old in s, old
    s = s.replace(old, new, 1)

# 波次验收行
old = "- [ ] **波次验收**：8 项全勾＋三线门禁绿＋QEMU 演示证据归档 → 进第 2 波。"
new = "- [x] **波次验收**：8 项全勾＋三线门禁绿＋QEMU 演示证据归档 → 进第 2 波。（2026-09-18 AI-B 走查：ktest 2961 绿/kcheck 0/后端 275 passed/tsc 0 错/vitest 2671；QEMU 证据 docs/acceptance/2026-09-17-任务65-保险箱内核侧/ + 2026-09-17-任务10-强拔演练/ + 2026-09-18-任务63-救援CLI-U盘自救援/）"
assert old in s
s = s.replace(old, new, 1)

io.open(p, 'w', encoding='utf-8', newline='\n').write(s)
s2 = io.open(p, encoding='utf-8').read()
assert s2.count('- [x] **步骤 1.') == 8
print('wave1 checked + verified')
