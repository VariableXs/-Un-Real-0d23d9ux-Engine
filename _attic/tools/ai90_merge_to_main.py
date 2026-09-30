# -*- coding: utf-8 -*-
"""把 AI-90 增补册以追加-only 方式并入总册（CoRun Varix STAR II · Unxreal.md），零覆写。"""
import io, sys, re
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")
BASE = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\Varix\CoRun Varix STAR II · Unxreal"
BOOK = BASE + r"\AI-90 · 治理线波次官 · 300项新功能增补册（B01–B15 · GOV90-001–GOV90-300）.md"
MAIN = BASE + r"\CoRun Varix STAR II · Unxreal.md"

marker = "AI-90 · 治理线波次官 · 300项新功能增补册"
main_txt = open(MAIN, encoding="utf-8").read()
if marker in main_txt:
    print("已并入，跳过（幂等）"); sys.exit(0)
book = open(BOOK, encoding="utf-8").read().rstrip() + "\n"
block = ("\n\n---\n\n# 增补卷 · AI-90 治理线波次官 · 300 项新功能增补册（B01–B15 · GOV90-001–GOV90-300）\n\n"
         "> 本节由独立增补册整卷并入（追加-only，零覆写既有内容）；独立账本体以同目录增补册文件为单一事实源，二者逐字一致由 _attic/tools/ai90_book_check.py 七断言看护。\n\n"
         + book)
with open(MAIN, "a", encoding="utf-8", newline="\n") as f:
    f.write(block)
chk = open(MAIN, encoding="utf-8").read()
assert marker in chk and chk.startswith(main_txt.rstrip("\n")[:1000]), "覆写检出"
print("并入完成：总册追加", len(block), "字符；头部千字零变更断言 PASS")
