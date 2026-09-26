# 批次八断言修正三连
import io

# 1) wow64: rest 保留原大小写 → 'kernel32.dll'（原串文件名就是小写）
p = "kernel/varix/src/compatstar/wow64.rs"
s = io.open(p, encoding="utf-8").read()
old = 'KERNEL32.dll"'
new = 'kernel32.dll"'
assert s.count(old) == 1, ("wow64", s.count(old))
io.open(p, "w", encoding="utf-8", newline="").write(s.replace(old, new))
print("wow64 fixed")

# 2) reghive: 删 idx0 后洞 {0,1,3,5} = 4
p = "kernel/varix/src/compatstar/reghive.rs"
s = io.open(p, encoding="utf-8").read()
old = "frag0 == 3 && frag1 == 3"
new = "frag0 == 3 && frag1 == 4"
assert s.count(old) == 1, ("reghive", s.count(old))
io.open(p, "w", encoding="utf-8", newline="").write(s.replace(old, new))
print("reghive fixed")

# 3) persrc: 好 = U+597D
p = "kernel/varix/src/compatstar/persrc.rs"
s = io.open(p, encoding="utf-8").read()
old = "0x597Cu16"
new = "0x597Du16"
assert s.count(old) == 1, ("persrc", s.count(old))
io.open(p, "w", encoding="utf-8", newline="").write(s.replace(old, new))
print("persrc fixed")
