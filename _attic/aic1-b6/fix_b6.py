# -*- coding: utf-8 -*-
# AI-C1 批次六修复 #2：四断言/实现修正
import io

# 1) mlangres: c3 锚改 0xFE=■；批次五 registry 断言 3→4（测试随状态演进）
p = "mlangres.rs"
s = io.open(p, encoding='utf-8').read()
old_def = "    let c3 = cp437_decode(0xF8);"
new_def = "    let c3 = cp437_decode(0xFE);"
assert s.count(old_def) == 1, ("c3def", s.count(old_def))
s = s.replace(old_def, new_def)
old = "        codepages_meeting_criterion() == 3"
new = "        codepages_meeting_criterion() == 4"
assert s.count(old) == 1, ("reg3", s.count(old))
s = s.replace(old, new)
io.open(p, 'w', encoding='utf-8', newline='').write(s)
print("mlangres fixed")

# 2) excface: 定位断言 idx 0（第一模块区间）
p = "excface.rs"
s = io.open(p, encoding='utf-8').read()
old = "        hit == Some(1) && miss.is_none(),"
new = "        hit == Some(0) && miss.is_none(),"
assert s.count(old) == 1, ("hit", s.count(old))
s = s.replace(old, new)
io.open(p, 'w', encoding='utf-8', newline='').write(s)
print("excface fixed")

# 3) gdiface: 渐变分母 w-1（末端恰达 c1）
p = "gdiface.rs"
s = io.open(p, encoding='utf-8').read()
old = "        let t = ((col * 1000) / w.max(1)) as u32; // 0..=1000 permille"
new = "        let t = ((col * 1000) / (w - 1).max(1)) as u32; // 0..=1000 permille（末列恰达 c1）"
assert s.count(old) == 1, ("t", s.count(old))
s = s.replace(old, new)
io.open(p, 'w', encoding='utf-8', newline='').write(s)
print("gdiface fixed")

# 4) peblend: EAT 偏移改 PE Spec 真实布局 + 测试构造同步
p = "peblend.rs"
s = io.open(p, encoding='utf-8').read()
old = """const EAT_OFF_FLAGS: usize = 16;
const EAT_OFF_NAME_RVA: usize = 24;
const EAT_OFF_ORD_BASE: usize = 32;
const EAT_OFF_ADDR_TABLE: usize = 36;
const EAT_OFF_NAME_PTRS: usize = 40;
const EAT_OFF_ORDINALS: usize = 44;
const EAT_OFF_COUNT: usize = 20;"""
new = """const EAT_OFF_NAME_RVA: usize = 16;
const EAT_OFF_ORD_BASE: usize = 20;
const EAT_OFF_COUNT: usize = 24;
const EAT_OFF_NAMES: usize = 28;
const EAT_OFF_ADDR_TABLE: usize = 32;
const EAT_OFF_NAME_PTRS: usize = 36;
const EAT_OFF_ORDINALS: usize = 40;"""
assert s.count(old) == 1, ("eat-consts", s.count(old))
s = s.replace(old, new)
old = """    let functions = read_u32(mapped, eat_off + EAT_OFF_COUNT)?;
    let name_rva = read_u32(mapped, eat_off + EAT_OFF_NAME_RVA)?;
    let ordinal_base = read_u32(mapped, eat_off + EAT_OFF_ORD_BASE)?;
    let addr_table_rva = read_u32(mapped, eat_off + EAT_OFF_ADDR_TABLE)?;
    let name_ptrs_rva = read_u32(mapped, eat_off + EAT_OFF_NAME_PTRS)?;
    let ordinals_rva = read_u32(mapped, eat_off + EAT_OFF_ORDINALS)?;
    let names = read_u32(mapped, eat_off + EAT_OFF_COUNT + 4)?;"""
new = """    let name_rva = read_u32(mapped, eat_off + EAT_OFF_NAME_RVA)?;
    let ordinal_base = read_u32(mapped, eat_off + EAT_OFF_ORD_BASE)?;
    let functions = read_u32(mapped, eat_off + EAT_OFF_COUNT)?;
    let names = read_u32(mapped, eat_off + EAT_OFF_NAMES)?;
    let addr_table_rva = read_u32(mapped, eat_off + EAT_OFF_ADDR_TABLE)?;
    let name_ptrs_rva = read_u32(mapped, eat_off + EAT_OFF_NAME_PTRS)?;
    let ordinals_rva = read_u32(mapped, eat_off + EAT_OFF_ORDINALS)?;"""
assert s.count(old) == 1, ("eat-parse", s.count(old))
s = s.replace(old, new)
old = """    m[eat_off + EAT_OFF_COUNT..eat_off + EAT_OFF_COUNT + 4].copy_from_slice(&1u32.to_le_bytes());
    m[eat_off + EAT_OFF_NAME_RVA..eat_off + EAT_OFF_NAME_RVA + 4].copy_from_slice(&0x300u32.to_le_bytes());
    m[eat_off + EAT_OFF_ORD_BASE..eat_off + EAT_OFF_ORD_BASE + 4].copy_from_slice(&1u32.to_le_bytes());
    m[eat_off + EAT_OFF_ADDR_TABLE..eat_off + EAT_OFF_ADDR_TABLE + 4].copy_from_slice(&0x360u32.to_le_bytes());
    m[eat_off + EAT_OFF_NAME_PTRS..eat_off + EAT_OFF_NAME_PTRS + 4].copy_from_slice(&0x340u32.to_le_bytes());
    m[eat_off + EAT_OFF_ORDINALS..eat_off + EAT_OFF_ORDINALS + 4].copy_from_slice(&0x380u32.to_le_bytes());
    m[eat_off + EAT_OFF_COUNT + 4..eat_off + EAT_OFF_COUNT + 8].copy_from_slice(&1u32.to_le_bytes());"""
new = """    m[eat_off + EAT_OFF_NAME_RVA..eat_off + EAT_OFF_NAME_RVA + 4].copy_from_slice(&0x300u32.to_le_bytes());
    m[eat_off + EAT_OFF_ORD_BASE..eat_off + EAT_OFF_ORD_BASE + 4].copy_from_slice(&1u32.to_le_bytes());
    m[eat_off + EAT_OFF_COUNT..eat_off + EAT_OFF_COUNT + 4].copy_from_slice(&1u32.to_le_bytes());
    m[eat_off + EAT_OFF_NAMES..eat_off + EAT_OFF_NAMES + 4].copy_from_slice(&1u32.to_le_bytes());
    m[eat_off + EAT_OFF_ADDR_TABLE..eat_off + EAT_OFF_ADDR_TABLE + 4].copy_from_slice(&0x360u32.to_le_bytes());
    m[eat_off + EAT_OFF_NAME_PTRS..eat_off + EAT_OFF_NAME_PTRS + 4].copy_from_slice(&0x340u32.to_le_bytes());
    m[eat_off + EAT_OFF_ORDINALS..eat_off + EAT_OFF_ORDINALS + 4].copy_from_slice(&0x380u32.to_le_bytes());"""
assert s.count(old) == 1, ("eat-build", s.count(old))
s = s.replace(old, new)
io.open(p, 'w', encoding='utf-8', newline='').write(s)
print("peblend fixed")
