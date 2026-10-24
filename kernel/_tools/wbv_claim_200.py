# -*- coding: utf-8 -*-
"""AI-WBV1022 领单：F3941-F4000 + F4015-F4154 共 200 卡标已领（流式写回，先备份）。"""
import io, os, re, shutil, time

SRC = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\dist\taskboard.md"
BAK = SRC + ".bak_1022_1945_wbv"
TMP = SRC + ".tmp_wbv"

OWNER = "AI-WBV1022"
NOW = "10-22 19:46"

CLAIM = set(range(3941, 4001)) | set(range(4015, 4155))

# 1. 备份
shutil.copy2(SRC, BAK)
assert os.path.getsize(BAK) > 0, "backup failed"

cur = None
patched = set()
with io.open(SRC, "r", encoding="utf-8", newline="") as fin, \
     io.open(TMP, "w", encoding="utf-8", newline="") as fout:
    for line in fin:
        m = re.match(r"^## \[VE-F(\d+)\]", line)
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
sz = os.path.getsize(SRC)
print("claimed:", len(patched), "size:", sz)
missing = CLAIM - patched
if missing:
    print("not-patched(likely already done):", sorted(missing)[:30], "... total", len(missing))
