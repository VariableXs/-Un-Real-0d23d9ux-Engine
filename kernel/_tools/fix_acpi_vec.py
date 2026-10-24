#!/usr/bin/env python3
"""为 unx0443~0449 补 use alloc::vec; 导入（内容驱动，免行号竞态）。"""
import io, os, re

BASE = os.path.join(os.path.dirname(__file__), "..", "varix", "src", "unxstar")
FILES = ["unx0443_rsdtwalk.rs", "unx0444_tblcsum.rs", "unx0445_madtirq.rs",
         "unx0446_hpetparse.rs", "unx0447_fadttiming.rs", "unx0448_mcfgecam.rs",
         "unx0449_dsdtloc.rs"]

for fn in FILES:
    p = os.path.join(BASE, fn)
    with io.open(p, "r", encoding="utf-8") as f:
        src = f.read()
    if re.search(r"^use alloc::vec;\s*$", src, re.M):
        print(fn, "已有 vec 宏导入，跳过")
        continue
    # 插在第一个 use 行前
    m = re.search(r"^use ", src, re.M)
    if not m:
        print(fn, "无 use 行，跳过（异常）")
        continue
    src = src[:m.start()] + "use alloc::vec;\n" + src[m.start():]
    with io.open(p, "w", encoding="utf-8", newline="") as f:
        f.write(src)
    print(fn, "已补 use alloc::vec;")
print("done")
