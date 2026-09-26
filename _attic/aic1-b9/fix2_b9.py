# 批次九第二轮修正：fsredir 根分隔逻辑 + 三处断言
import io

# 1) fsredir: 去掉 i==0 的根分隔符推入——统一用段间分隔；单盘符根补尾分隔。
p = "kernel/varix/src/compatstar/fsredir.rs"
s = io.open(p, encoding="utf-8").read()
old = """    for (i, seg) in segs.iter().enumerate() {
        if i == 0 {
            out.extend_from_slice(seg.as_bytes());
            if seg.ends_with(':') {
                out.push(b'\\\\'); // 盘符根形态 `C:\\`
            }
        } else {
            out.push(b'\\\\');
            out.extend_from_slice(seg.as_bytes());
        }
    }
    Ok(())"""
new = """    for (i, seg) in segs.iter().enumerate() {
        if i > 0 {
            out.push(b'\\\\');
        }
        out.extend_from_slice(seg.as_bytes());
    }
    // 单盘符根（无后续段）保留 `C:\\` 形态——多段时盘符后的分隔由 i=1 补，
    // 不在此重复（重复即双反斜杠——批次九缺陷 #1 的教训落码处）。
    if segs.len() == 1 && segs[0].ends_with(':') {
        out.push(b'\\\\');
    }
    Ok(())"""
assert s.count(old) == 1, ("fsredir", s.count(old))
io.open(p, "w", encoding="utf-8", newline="").write(s.replace(old, new))
print("fsredir fixed")

# 2) peblend: reloc 0xF000+0x1000 也越界（>0xA000）——断言 out==2 并修正注释
p = "kernel/varix/src/compatstar/peblend.rs"
s = io.open(p, encoding="utf-8").read()
old = """        v[5] = (0xF000, 0x1000); // BASE reloc——恰好触底
        v[6] = (0x9000, 0x2000); // debug——越界（rva+size > 0xA000 假设）"""
new = """        v[5] = (0xF000, 0x1000); // BASE reloc——0xF000+0x1000 > 0xA000，越界
        v[6] = (0x9000, 0x2000); // debug——越界"""
assert s.count(old) == 1, ("peblend-comment", s.count(old))
s = s.replace(old, new)
old = """        rep.present == 4
            && rep.zero_entries == 12
            && rep.out_of_image == 1, // 只有 debug 项越界；reloc 恰好在界内"""
new = """        rep.present == 4
            && rep.zero_entries == 12
            && rep.out_of_image == 2, // reloc 与 debug 双双越界"""
assert s.count(old) == 1, ("peblend-assert", s.count(old))
io.open(p, "w", encoding="utf-8", newline="").write(s.replace(old, new))
print("peblend fixed")

# 3) wow64: softwarex 前缀陷阱——strip 后尾段必须空或以 \ 起，否则域外
p = "kernel/varix/src/compatstar/wow64.rs"
s = io.open(p, encoding="utf-8").read()
old = """    let Some(after) = rest.strip_prefix(sw) else {
        return RegRedirect::Outside;
    };
    if after.strip_prefix("\\\\classes").map_or(false, |tail| tail.is_empty() || tail.starts_with('\\\\')) {"""
new = """    let Some(after) = rest.strip_prefix(sw) else {
        return RegRedirect::Outside;
    };
    // `softwarex` 前缀陷阱：strip 成功但尾巴不是键分隔 = 不同键，域外。
    if !(after.is_empty() || after.starts_with('\\\\')) {
        return RegRedirect::Outside;
    }
    if after.strip_prefix("\\\\classes").map_or(false, |tail| tail.is_empty() || tail.starts_with('\\\\')) {"""
assert s.count(old) == 1, ("wow64", s.count(old))
io.open(p, "w", encoding="utf-8", newline="").write(s.replace(old, new))
print("wow64 fixed")

# 4) reghive: free_cell(16,8) 即刻与 (24,232) 合并 → segs_before == 2
p = "kernel/varix/src/compatstar/reghive.rs"
s = io.open(p, encoding="utf-8").read()
old = "        segs_before == 3 && arena.free_segments() == 1 && arena.free_total() == 256,"
new = "        segs_before == 2 && arena.free_segments() == 1 && arena.free_total() == 256,"
assert s.count(old) == 1, ("reghive", s.count(old))
io.open(p, "w", encoding="utf-8", newline="").write(s.replace(old, new))
print("reghive fixed")
