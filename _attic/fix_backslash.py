# -*- coding: utf-8 -*-
"""修正 kinfo 续行：双反斜杠 → 单反斜杠（Rust 宏行继续符）"""
import io

p = r"kernel/varix/src/quota.rs"
s = io.open(p, encoding="utf-8").read()
# 文件实际内容 = 两个反斜杠 + 换行；目标 = 单个反斜杠 + 换行
bad = "\\\\\n                 reclaimed="
good = "\\\n                 reclaimed="
assert bad in s, "bad pattern not found"
s = s.replace(bad, good, 1)
io.open(p, "w", encoding="utf-8", newline="").write(s)
s2 = io.open(p, encoding="utf-8").read()
assert bad not in s2 and good in s2, "rewrite failed"
print("backslash fixed ok")
