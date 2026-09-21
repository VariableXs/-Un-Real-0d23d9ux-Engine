# -*- coding: utf-8 -*-
"""AI-4 ushell 笔误修复（disp 诊断行 p 推进 + OpenSelFile 死代码段）。"""
import io

P = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\user\ushell\src\main.rs"

with io.open(P, "r", encoding="utf-8", newline="") as f:
    src = f.read()

old1 = "        let d = u64_bytes(h as u64, &mut nb);\n        line[p..p + d].copy_from_slice(&nb[..d]);\n        p += 1;\n        line[p] = b'\\n';\n        p += 1;"
new1 = "        let d = u64_bytes(h as u64, &mut nb);\n        line[p..p + d].copy_from_slice(&nb[..d]);\n        p += d;\n        line[p] = b'\\n';\n        p += 1;"

old2 = "                if phase == 4 && setting_sel == 1 {\n                    click = Click::SetToggle;\n                }\n            }"
new2 = "            }"

n = 0
if old1 in src:
    src = src.replace(old1, new1, 1)
    n += 1
else:
    print("SKIP old1")
if old2 in src:
    src = src.replace(old2, new2, 1)
    n += 1
else:
    print("SKIP old2")

with io.open(P, "w", encoding="utf-8", newline="") as f:
    f.write(src)

with io.open(P, "r", encoding="utf-8") as f:
    back = f.read()
ok = new1 in back and old2 not in back
print("applied:", n, "| verify:", ok)
import sys
sys.exit(0 if ok else 1)
