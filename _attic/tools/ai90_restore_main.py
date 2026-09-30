# -*- coding: utf-8 -*-
"""总册重建：恢复被并行覆写吞掉的他域登记段。
final = old(7e64cf9a^) − AI-89旧前缀登记段 + AI-90整块 + AI-89新前缀登记段(HEAD)
验证：final 与 HEAD 的差集 == {AI-90块} ∓ 前缀块，即 HEAD 缺的他域内容全部回归。
"""
import io, sys, os
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")

old = open("_attic/main_old_7e64.md", encoding="utf-8").read()
head = open("_attic/main_new_head.md", encoding="utf-8").read()

def cut_block(txt, start_marker, end_marker=None):
    """取 [start_marker 行起, end_marker 或文末) 的整块。"""
    i = txt.find(start_marker)
    assert i != -1, start_marker
    j = txt.find(end_marker, i + len(start_marker)) if end_marker else -1
    return txt[i:j] if j != -1 else txt[i:]

# 1) old 中剔除 AI-89 旧前缀登记段（块起于其登记段标题，止于下一个一级/增补卷标题）
OLD89_START = "## AI-89 · 镜像官 · 300 项新功能增补册登记段（G01–G15 · GV89001–GV89300 · append-only 追加）"
i = old.find(OLD89_START)
assert i != -1, "old AI-89 旧前缀段未找到"
# 下一块起点：其后第一个行首 '#'
import re
m = re.search(r"^# ", old[i + len(OLD89_START):], re.M)
j = i + len(OLD89_START) + m.start()
old_clean = old[:i] + old[j:]

# 2) 我的 AI-90 块（来自 HEAD）
AI90_START = "# 增补卷 · AI-90 治理线波次官 · 300 项新功能增补册"
ai90 = cut_block(head, AI90_START, "\n\x00")  # 到文末
# AI-90 块是 HEAD 文件最后一块？核对：AI-89 新前缀段在 AI-90 之前还是之后
pos_89 = head.find("UNX-GOV-89001")
pos_90 = head.find(AI90_START)
print("HEAD 内 AI-89新前缀段位置:", pos_89, "AI-90 块位置:", pos_90, "(AI-90 在后)" if pos_90 > pos_89 else "(AI-90 在前)")
if pos_90 > pos_89:
    ai89_new = cut_block(head, "## AI-89 · 镜像官 · 300 项新功能增补册登记段（G01–G15 · UNX-GOV-89001")
    # ai89_new 可能吞到文末含 AI-90 块——截到 AI-90 块前
    k = ai89_new.find(AI90_START)
    if k != -1:
        ai89_new = ai89_new[:k]
    ai90 = cut_block(head, AI90_START)
    final = old_clean.rstrip("\n") + "\n\n" + ai89_new.strip("\n") + "\n\n" + ai90.strip("\n") + "\n"
else:
    ai90 = cut_block(head, AI90_START, "## AI-89") if head.find("## AI-89", pos_90) != -1 else cut_block(head, AI90_START)
    final = old_clean.rstrip("\n") + "\n\n" + ai90.strip("\n") + "\n"

# 3) 验证：HEAD 相对 final 的净缺失行（排除 AI-89 前缀勘误的合法差异）应为零他域缺失
old_lines = set(final.splitlines())
head_missing = [l for l in head.splitlines() if l not in old_lines]
final_missing = [l for l in final.splitlines() if l not in set(head.splitlines())]
print("HEAD 有而 final 无:", len(head_missing))
print("final 有而 HEAD 无:", len(final_missing))
for l in final_missing[:5]:
    print("  final独有:", repr(l[:100]))
for l in head_missing[:5]:
    print("  head独有:", repr(l[:100]))

# 4) 关键回归断言：他域登记段标题必须在 final 中
for kw in ["# AI-81 · 台账治理域 · 300 项新功能增补册", "# AI-85 · 治理线判据审计",
           "# AI-79 · UNX-P4 上游跟随", "# AI-78 · UNX-P3 断代防治与传承 · 300 项新功能增补册",
           "# 增补卷 · AI-86 · 红线官", "# AI-84 · 治理线 · 防重审计",
           "# AI-76 · UNX-P1 断点续写产线制度化", AI90_START]:
    assert kw in final, "回归缺失: " + kw
print("八块回归断言 PASS")

open("_attic/main_rebuilt.md", "w", encoding="utf-8", newline="\n").write(final)
print("重建件落盘 _attic/main_rebuilt.md，行数:", final.count("\n"))
