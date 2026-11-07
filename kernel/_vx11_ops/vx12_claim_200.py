# -*- coding: utf-8 -*-
"""AI-VX12-1106 领单：F81001-F81200 共 200 卡（CoRun·UNX-F2-B40 台账域）。
流式写回，先备份；只把「待领」改为「已领」并填领取人/时间，不碰他人卡。"""
import io
import re
import shutil
import os
import datetime

SRC = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\dist\taskboard.md"
BAK = SRC + ".bak_vx12_claim"
TMP = SRC + ".tmp_vx12"

OWNER = "AI-VX12-1106"
NOW = datetime.datetime.now().strftime("%m-%d %H:%M")

CLAIM = set(range(81001, 81201))

shutil.copy2(SRC, BAK)
assert os.path.getsize(BAK) > 0, "backup failed"

cur = None
patched = set()
with io.open(SRC, "r", encoding="utf-8", newline="") as fin, \
     io.open(TMP, "w", encoding="utf-8", newline="") as fout:
    for line in fin:
        m = re.match(r"^## \[UNX-F(\d+)\]", line)
        if m:
            cur = int(m.group(1))
        if cur in CLAIM:
            if line.startswith("- 状态: 待领"):
                line = "- 状态: 已领\n"
                patched.add(cur)
            elif line.startswith("- 领取人: ") and line.rstrip("\r\n") == "- 领取人: ":
                line = "- 领取人: %s\n" % OWNER
            elif line.startswith("- 时间: ") and len(line.rstrip("\r\n")) <= len("- 时间: ") + 1:
                line = "- 时间: %s\n" % NOW
        fout.write(line)

os.replace(TMP, SRC)
print("claimed:", len(patched), "size:", os.path.getsize(SRC))
missing = CLAIM - patched
if missing:
    print("not-patched(可能已被他人领/已完成):", sorted(missing)[:30], "total", len(missing))
