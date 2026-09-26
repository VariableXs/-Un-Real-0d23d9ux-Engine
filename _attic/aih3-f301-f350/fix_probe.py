# -*- coding: utf-8 -*-
"""临时探针修正：去掉跨模块引用。"""
import io

p = "kernel/varix/src/h3star/copypath.rs"
s = io.open(p, encoding="utf-8").read()
old = '''        println!(
            "NEAR 50={} 120={} 500={}",
            MemoryAudit::near_limit(50, 4000),
            MemoryAudit::near_limit(120, 4000),
            MemoryAudit::near_limit(500, 4000)
        );
'''
assert old in s
s = s.replace(old, "")
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("probe fixed")
