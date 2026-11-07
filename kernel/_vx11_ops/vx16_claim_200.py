# -*- coding: utf-8 -*-
"""AI-VX11-1107 VX16 领单：F52401-F52600 共 200 卡。
流式写回，先备份；只把「待领」改为「已领」并填领取人/时间，不碰他人卡。"""
import io, re, shutil, datetime, os

SRC = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\dist\taskboard.md"
OWNER = "AI-VX11-1107(VX16)"
NOW = datetime.datetime.now().strftime("%m-%d %H:%M")
CLAIM = set(range(52401, 52601))

shutil.copy2(SRC, SRC + ".bak_vx16_claim")
assert os.path.getsize(SRC + ".bak_vx16_claim") > 0, "backup failed"

cur = None
patched = set()
with io.open(SRC, "r", encoding="utf-8", newline="") as fin, \
     io.open(SRC + ".tmp_vx16", "w", encoding="utf-8", newline="") as fout:
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
            elif line.startswith("- 时间: ") and line.rstrip("\r\n") == "- 时间: ":
                line = "- 时间: %s\n" % NOW
        fout.write(line)

os.replace(SRC + ".tmp_vx16", SRC)
print("claimed:", len(patched))
missing = CLAIM - patched
if missing:
    print("not-patched:", sorted(missing)[:20], "total", len(missing))
else:
    print("all 200 claimed OK")
