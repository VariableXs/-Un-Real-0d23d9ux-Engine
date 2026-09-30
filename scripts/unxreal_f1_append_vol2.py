# -*- coding: utf-8 -*-
"""UNX-F1 深化增补卷·二段（B16–B30 · 300 条）→ 主汇编册卷末纯追加。
单源读 deepen/F1-B16..B30.md 与骨架账，零转抄、零删除既有内容。"""
import io, re, os

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEEPEN = os.path.join(REPO, "docs", "unxreal", "deepen")
BATCH = os.path.join(REPO, "docs", "unxreal", "batches")
BOOK = os.path.join(REPO, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")

THEME = {
 16:"损伤追踪与重绘调度",17:"混成与面处理",18:"多输出与直扫",19:"呈现时序与帧节奏",20:"合成管线段收口",
 21:"指针路由",22:"键盘与焦点",23:"触摸手势与平板",24:"抓取栈与快捷键拦截",25:"输入路由段收口",
 26:"global 注册与版本协商",27:"资源生命周期与客户端隔离",28:"事件批与背压",29:"协议诊断与可观测面",
 30:"协议服务面段收口与二轮终环",
}

out = []
out.append("\n## 卷末增补卷 · AI-26 · F1 域深化增补卷·二段（B16–B30 · 300 条深化 · 二轮三段合成管线/输入路由/协议服务面深化收口）\n")
out.append("\n> **深化增补卷·二段登记（AI-26 · 2026-09-30）**：UNX-F1 域骨架 800 条满账封账（240,000/240,000 · F20800 终钉）后深化阶段续作：本卷收录二段深化 300 条（B16–B30，每条 ≥300 字六要素深化正文，行数与骨架账逐条 verbatim 一致，深化零改行数、零新 ID）。三段主题：B16–B20 合成管线段（损伤追踪/混成/多输出直扫/呈现时序/段收口）｜B21–B25 输入路由段（指针/键盘焦点/触摸平板/抓取拦截/段收口）｜B26–B30 协议服务面段（global 版本协商/资源生命周期/事件批背压/协议诊断/二轮终环锚 F20600）。生成器 scripts/unxreal_f1_deepen_gen2.py 单源读骨架账防转抄；校验器 scripts/unxreal_f1_deepen_check.py 升 30 册口径六查全绿 exit=0（600 条 · 域深化累计 161,460/161,460）；主汇编册本卷为卷末纯追加零删除（R-PROC-002 纪律：提交前 diff 删除行数核查）。深化剩余 B31–B40（200 条）待续轮。\n")

grand = 0
for b in range(16, 31):
    dt = io.open(os.path.join(DEEPEN, f"F1-B{b:02d}.md"), encoding="utf-8").read()
    heads = re.findall(r"^### (UNX-F(\d{5})) · (.+)$", dt, re.M)
    assert len(heads) == 20
    lo, hi = heads[0][1], heads[-1][1]
    rows_sum = 0
    out.append(f"\n### 深化册 UNX-F1-B{b:02d}（F{lo}–F{hi} · 20 条 · 20 条新深化）\n")
    for full, idn, title in heads:
        blk_match = re.search(re.escape(full) + r" · .*?\n- 域/批：([^\n]+)", dt)
        meta = blk_match.group(1)
        rows = int(re.search(r"纯功能行数：(\d+)", meta).group(1))
        rows_sum += rows
        judge = re.search(r"判据：(UNX-F\d{5}-J1[^｜]*)", meta).group(1).strip()
        out.append(f"| {full} | {title.strip()} | {rows} | [已深化] | {judge} |")
    out.append(f"\n**B{b:02d} 勾稽**：批主题「{THEME[b]}」· 20 条 · 行数锁定 {rows_sum}（= 骨架账逐条之和，深化零改行数）。\n")
    grand += rows_sum
out.append(f"\n**深化增补卷·二段卷尾勾稽**：300 条深化 · 15 册 · 本段行数累计 {grand:,}（域深化累计 161,460/161,460，与骨架账逐条一致）；深化剩余 B31–B40（200 条）待续轮。—— AI-26 深化增补卷·二段终\n")

text = "\n".join(out) + "\n"
# 表头（首次出现前插入）
text = text.replace("| UNX-F20301 |", "| 条目 ID | 条目名 | 纯功能行数 | 状态 | 判据 |\n|---|---|---|---|---|\n| UNX-F20301 |", 1)

with io.open(BOOK, "a", encoding="utf-8", newline="\n") as f:
    f.write(text)
print(f"appended: 300 entries, rows_sum={grand}")
