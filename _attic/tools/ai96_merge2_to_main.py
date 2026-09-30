# -*- coding: utf-8 -*-
"""AI-96 续卷以追加-only 方式并入总册，零覆写。"""
import io, sys
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")
BASE = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\Varix\CoRun Varix STAR II · Unxreal"
BOOK = BASE + r"\AI-96 · 覆盖率统计官 · 500项新功能增补册续卷（B16–B40 · GOV96-301–GOV96-800）.md"
MAIN = BASE + r"\CoRun Varix STAR II · Unxreal.md"
marker = "增补卷 · AI-96 治理线覆盖率统计官 · 500 项新功能增补册续卷"
main_txt = open(MAIN, encoding="utf-8").read()
if marker in main_txt:
    print("已并入，跳过（幂等）"); sys.exit(0)
book = open(BOOK, encoding="utf-8").read().rstrip() + "\n"
block = ("\n\n---\n\n# " + marker + "（B16–B40 · GOV96-301–GOV96-800）\n\n"
         "> 本节由独立续卷整卷并入（追加-only，零覆写既有内容）；独立账本体以同目录续卷文件为单一事实源，二者逐字一致由 _attic/tools/ai96_book_check2.py 九断言看护。\n\n"
         + book)
with open(MAIN, "a", encoding="utf-8", newline="\n") as f:
    f.write(block)
chk = open(MAIN, encoding="utf-8").read()
assert marker in chk and chk.startswith(main_txt[:1000]), "覆写检出"
print("并入完成：总册追加", len(block), "字符；头部千字零变更断言 PASS")
