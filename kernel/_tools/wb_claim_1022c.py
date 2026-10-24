# -*- coding: utf-8 -*-
"""任务板领取登记：F3936-F3940 五卡标已领（流式写回）。"""
import io, os, re

SRC = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\dist\taskboard.md"
TMP = SRC + ".tmp_wb1022c"

CLAIM = {"VE-F3936", "VE-F3937", "VE-F3938", "VE-F3939", "VE-F3940"}
NOW = "10-22 19:35"
cur = None
buf = []
patched = set()

# 先读一遍找状态行，再流式重写。
# 简化：单遍处理，遇到目标卡的 "- 状态: 待领" 行改为已领；"- 领取人" 行填 AI-WB-1022；
# "- 时间" 行填 NOW。若缺字段则不补（保守）。
with io.open(SRC, "r", encoding="utf-8", newline="") as fin, \
     io.open(TMP, "w", encoding="utf-8", newline="") as fout:
    for line in fin:
        m = re.match(r"^## \[(VE-F\d+)\]", line)
        if m:
            cur = m.group(1)
        if cur in CLAIM:
            if line.startswith("- 状态: 待领"):
                line = "- 状态: 已领\n"
                patched.add(cur)
            elif line.startswith("- 领取人: ") and line.strip() == "- 领取人: ":
                line = "- 领取人: AI-WB-1022\n"
            elif line.startswith("- 时间: ") and line.strip() == "- 时间: ":
                line = f"- 时间: {NOW}\n"
        fout.write(line)

os.replace(TMP, SRC)
print("claimed:", len(patched), sorted(patched))
