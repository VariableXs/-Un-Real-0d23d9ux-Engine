# -*- coding: utf-8 -*-
"""按行号修正账本表头（验证基线/源码规模/单测三行）。"""
import io

p = "_attic/aih3-f301-f350/行数对账与缺陷账本.md"
lines = io.open(p, encoding="utf-8").read().splitlines(keepends=True)

for i, l in enumerate(lines):
    if l.startswith("| 验证基线 |"):
        lines[i] = ("| 验证基线 | **421 通过 / 0 失败 / 0 警告**（域内单测实数 = cargo test 424 − 3 舱内探针；"
                    "隔离舱 `_attic/aih3-f301-f350/isolation/` 经 `#[path]` 直挂实测，深化批次十二收口复核） |\n")
    if l.startswith("| 源码规模 |"):
        lines[i] = ("| 源码规模 | 总 **30,659** 行；纯功能 **18,835** 行（口径：总行 − 空行 − 注释行 − 测试区行；"
                    "其中 50 项 18,201 + hbase 531 + 域聚合器 103。**计数器 v2 程序化实数**（字符串感知——见 D-30），"
                    "脚本随仓 `_attic/aih3-f301-f350/count_pure_v2.py`） |\n")
    if l.startswith("| 域内单元测试 |"):
        lines[i] = ("| 域内单元测试 | **421 个**（cargo test 424 − 3 舱内探针） | 宿主 cargo test 直跑（注入钟确定复现）；"
                    "批次十一收口 408 → 本批 +13 |\n")

io.open(p, "w", encoding="utf-8", newline="\n").write("".join(lines))
print("header fixed by line number")
