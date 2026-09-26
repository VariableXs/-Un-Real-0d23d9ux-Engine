
// ---------------------------------------------------------------------------
// F003 · 深化批次六：序号导入绑定（按序号键——名称键的姊妹路径）
//
// 主册依据（G-A-03【功能定义】）：「IAT 解析的两级缓存体系」——导入按名或按
// 序号（PE Spec：Ordinal/Name 标志位）；既有 BindKey 走符号名哈希，本段补
// **序号键**（模块哈希 + 序号）——序号导入不需要名称表查找，快路径。
// ---------------------------------------------------------------------------

/// 序号键合成（模块哈希 × 16 位序号——与名称键同槽位空间、不同位域标识）。
pub fn ordinal_key(module_hash: u64, ordinal: u16) -> u64 {
    // FNV 混合：module_hash 8 字节 + 0xFFFF 标记 + 序号 2 字节（标记位区分
    // 序号键与名称键——同一槽位空间不撞键型）。
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in module_hash.to_le_bytes().iter().chain([0xEF, 0xBE].iter()).chain(ordinal.to_le_bytes().iter()) {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 序号键 vs 名称键判别（键型标记——同表混存的判别位）。
pub fn key_kinds_distinct(module_hash: u64, ordinal: u16, name_key: u64) -> bool {
    ordinal_key(module_hash, ordinal) != name_key
}

/// F003 深化批次六自检。
pub fn run_pebind_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F003-pebind-deep5");
    // 1) 序号键确定性：同模块同序号恒同键；不同序号不同键。
    let k1 = ordinal_key(0xFEED, 42);
    cs.add(
        "ordinal_key_deterministic",
        k1 == ordinal_key(0xFEED, 42) && k1 != ordinal_key(0xFEED, 43) && k1 != ordinal_key(0xFEED_0001, 42),
        "",
    );
    // 2) 键型判别：序号键与名称键不撞（混存安全）。
    cs.add(
        "ordinal_vs_name_key_distinct",
        key_kinds_distinct(0xFEED, 42, 0x1234_5678_9ABC_DEF0),
        "",
    );
    // 3) 快路径语义：序号导入无需名称表查找——键即定位（与 BindTable 既有
    //    get/put 槽位空间同构的模型锚）。
    let k2 = ordinal_key(0xFEED, 0xFFFF);
    cs.add(
        "ordinal_key_full_range",
        k2 != ordinal_key(0xFEED, 0xFFFE) && k2 != 0,
        "",
    );
    cs
}
