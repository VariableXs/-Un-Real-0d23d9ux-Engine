# -*- coding: utf-8 -*-
"""全仓十六进制字面量扫描，为 AI-WBV1022 的 200 个新模块分配无冲突 0x40 段位窗口。"""
import os, re

pat = re.compile(r"0[xX][0-9A-Fa-f]{2,6}")
used = set()
root_dir = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\kernel\varix\src"
for root, dirs, files in os.walk(root_dir):
    for fn in files:
        if fn.endswith(".rs"):
            try:
                with open(os.path.join(root, fn), encoding="utf-8", errors="ignore") as f:
                    for tok in pat.findall(f.read()):
                        used.add(int(tok, 16))
            except OSError:
                pass

print("used values:", len(used))
free = []
for base in range(0x0200, 0x10000, 0x40):
    if all((base + i) not in used for i in range(16)):
        free.append(base)
print("free 0x40 windows:", len(free))
sel = free[:200]
if len(sel) < 200:
    raise SystemExit("not enough windows: %d" % len(sel))
print("first:", hex(sel[0]), "last:", hex(sel[-1]))
out = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\kernel\_tools\wbv_segs.txt"
with open(out, "w") as f:
    for i, b in enumerate(sel):
        f.write("%d 0x%04X\n" % (i, b))
print("written:", out)
