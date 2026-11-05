# -*- coding: utf-8 -*-
"""VX9 任务板回写器（工具脚本）。
用法：python -I vx9_close.py <start> <end> <结果摘要模板，{id} 占位>
只改 [start,end] 区间内状态为「已领」且领取人为 AI-VX9-1105 的卡：
  状态→已完成；结果→模板填 id；时间→当前时间。"""
import io, os, re, sys, shutil, datetime

SRC = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\dist\taskboard.md"
TMP = SRC + ".tmp_vx9"
OWNER = "AI-VX9-1105"
NOW = datetime.datetime.now().strftime("%m-%d %H:%M")

start, end = int(sys.argv[1]), int(sys.argv[2])
tmpl = sys.argv[3] if len(sys.argv) > 3 else "VX9 完成：{id} 模块落地"
want = set(range(start, end + 1))

cur = None
closed = set()
with io.open(SRC, "r", encoding="utf-8", newline="") as fin, \
     io.open(TMP, "w", encoding="utf-8", newline="") as fout:
    for line in fin:
        m = re.match(r"^## \[UNX-F(\d+)\]", line)
        if m:
            cur = int(m.group(1))
        if cur in want:
            if line.startswith("- 状态: 已领"):
                line = "- 状态: 已完成\n"
                closed.add(cur)
            elif line.startswith("- 结果: ") and len(line.rstrip("\r\n")) <= len("- 结果: ") + 1:
                line = "- 结果: %s\n" % tmpl.replace("{id}", str(cur))
            elif line.startswith("- 时间: "):
                line = "- 时间: %s\n" % NOW
        fout.write(line)

os.replace(TMP, SRC)
print("closed:", len(closed), "/", len(want))
missing = want - closed
if missing:
    print("not-closed(非本人已领或已完成):", sorted(missing)[:20])
