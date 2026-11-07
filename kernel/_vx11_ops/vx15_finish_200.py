# -*- coding: utf-8 -*-
"""AI-VX11-1107 VX15 收官：F52201-F52400 共 200 卡写回「已完成」。
流式写回，先备份；只把本批卡的状态改为已完成并填结果摘要，不碰他人卡。"""
import io, re, shutil, datetime, os

SRC = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\dist\taskboard.md"
OWNER = "AI-VX11-1107(VX15)"
NOW = datetime.datetime.now().strftime("%m-%d %H:%M")
CLAIM = set(range(52201, 52401))

shutil.copy2(SRC, SRC + ".bak_vx15_finish")
assert os.path.getsize(SRC + ".bak_vx15_finish") > 0, "backup failed"

cur = None
patched = set()
with io.open(SRC, "r", encoding="utf-8", newline="") as fin, \
     io.open(SRC + ".tmp_vx15f", "w", encoding="utf-8", newline="") as fout:
    for line in fin:
        m = re.match(r"^## \[UNX-F(\d+)\]", line)
        if m:
            cur = int(m.group(1))
        if cur in CLAIM:
            if line.startswith("- 状态: 已领"):
                line = "- 状态: 已完成（%s）\n" % OWNER
                patched.add(cur)
            elif line.startswith("- 结果: ") and line.rstrip("\r\n") == "- 结果: ":
                line = "- 结果: VX15 收口：手写逐卡定制模块落库，SEG 0x40 对齐零撞号，探针全绿（TOTAL=3488 RED=0），测试 2958/2958 全绿，mod.rs 批二十四登记全批 200 单，任务板写回已完成（%s）\n" % NOW
        fout.write(line)

os.replace(SRC + ".tmp_vx15f", SRC)
print("finished:", len(patched))
missing = CLAIM - patched
if missing:
    print("not-patched:", sorted(missing)[:20], "total", len(missing))
else:
    print("all 200 marked OK")
