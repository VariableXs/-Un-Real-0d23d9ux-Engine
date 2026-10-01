# -*- coding: utf-8 -*-
"""AI-105 · 主汇编册增补卷二次归位（R-PROC-002 后基于新态重放，判例承 AI-26/102）。
守卫式纯追加：读现态 → W90-Q5 前缀零命中校验 → 前缀哈希记录 → 追加 → 回读断言。"""
import hashlib
import io
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
os.chdir(ROOT)

BOOK = "docs/Varix/CoRun Varix STAR II · Unxreal/AI-105 · Q5 · 300项新功能增补册（B01–B15 · W90-Q5-001–300）.md"
MAIN = "docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md"

book = io.open(BOOK, encoding="utf-8").read()
assert book.startswith("# AI-105 · Q5 · 300项新功能增补册")
body = book.split("\n", 1)[1].lstrip("\n")  # 去书名行，保正文

header = (
    "\n---\n\n"
    "> **增补卷登记（AI-105 · 2026-10-01 · 本轮 300 项明令 · W90 兼容专项）**：卷末追加"
    "《增补卷 · AI-105 · W90-Q5 UWP/MSIX 现代应用模型 首产段》——W90-Q5-001–300 共 300 项新功能"
    "（15 批 × 20 条，连续零跳号，每批 6,000 行、全段 90,000 行，域账 90,000/240,000（37.5%），"
    "状态列统一「骨架」不冒充深化），增补册源文件 docs/Varix/CoRun Varix STAR II · Unxreal/"
    "AI-105 · Q5 · 300项新功能增补册（B01–B15 · W90-Q5-001–300）.md；判据全部围绕 varix 内核链"
    "（pkgstore/fsview/reggate/signchain/sec·secgate/proc·proc::job/kvsrv/syslogd·observ/task·power/"
    "ktest M-Q5-###/kcheck 0），T1/T2/T3 三档全给断言数字与执行入口，实弹随 CW-07～12 波次窗闸门补测；"
    "生成器 docs/unxreal/gen/_q5_firstprod.py 五断言 ALL PASS exit=0；不占他域账、不改 64,000 公理"
    "（W90 兼容专项走十二卷台账分册口径，纯登记视图）、纯追加零删除。详见根台账本会话条目。\n\n"
    "## 增补卷 · AI-105 · W90-Q5 UWP/MSIX 现代应用模型 首产段（B01–B15 · W90-Q5-001–300 · 300 条）\n\n"
)

block = (header + body).encode("utf-8")

with open(MAIN, "rb") as f:
    old = f.read()
assert "W90-Q5-001".encode() not in old, "主册已含 W90-Q5 段——无需归位（幂等退出）"
old_sha = hashlib.sha256(old).hexdigest()
new = old + block
assert new.startswith(old), "前缀不变量破坏——拒绝落件"
with open(MAIN, "wb") as f:
    f.write(new)
with open(MAIN, "rb") as f:
    back = f.read()
assert back == new and back.startswith(old), "回读断言失败"
cnt = back.count("W90-Q5-".encode())
print(f"主册增补卷二次归位完成：前缀 {old_sha[:16]} 保位；现 {len(back)} 字节；W90-Q5- 命中 {cnt} 行")
