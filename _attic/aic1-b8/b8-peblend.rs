
// ---------------------------------------------------------------------------
// F002 · 深化批次八：EAT 按序号解析（GetProcAddress 序号路径——批次六做了
// 按名三表链，本段补序号直达链：index = ordinal - ordinal_base）。
//
// 依据：PE Spec 8.4——序号入参落在 [ordinal_base, ordinal_base+NumberOfFunctions)
// 才合法；直接以 index 索引 AddressOfFunctions。本函数在平坦缓冲上操作
// （测试域 RVA==offset 恒等映射；真实装载走 BlendImage 的 rva_to_off）。
// ---------------------------------------------------------------------------

fn eat_u32_at(buf: &[u8], off: usize) -> Option<u32> {
    if off + 4 > buf.len() {
        return None;
    }
    Some(u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]))
}

/// 按序号取导出函数 RVA（越界 / 空表如实 None——不猜不越权）。
pub fn eat_resolve_by_ordinal(
    mapped: &[u8],
    eat_off: usize,
    ordinal: u32,
) -> Option<u32> {
    let ordinal_base = eat_u32_at(mapped, eat_off + EAT_OFF_ORD_BASE)?;
    let count = eat_u32_at(mapped, eat_off + EAT_OFF_COUNT)?;
    if count == 0 || ordinal < ordinal_base || ordinal - ordinal_base >= count {
        return None;
    }
    let idx = (ordinal - ordinal_base) as usize;
    let addr_table = eat_u32_at(mapped, eat_off + EAT_OFF_ADDR_TABLE)? as usize;
    eat_u32_at(mapped, addr_table + idx * 4)
}

/// F002 深化批次八自检（复用批次六的最小导出镜像布局：基 1、函数 1 个、
/// 地址表 @0x360）。
fn run_peblend_deep7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F002-peblend-deep7");
    let mut m = alloc::vec![0u8; 0x800];
    let eat_off = 0x2A0;
    m[eat_off + EAT_OFF_ORD_BASE..eat_off + EAT_OFF_ORD_BASE + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    m[eat_off + EAT_OFF_COUNT..eat_off + EAT_OFF_COUNT + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    m[eat_off + EAT_OFF_ADDR_TABLE..eat_off + EAT_OFF_ADDR_TABLE + 4]
        .copy_from_slice(&0x360u32.to_le_bytes());
    m[0x360..0x364].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    // 1) 基内序号直达：ord=1 → 表[0]。
    let hit = eat_resolve_by_ordinal(&m, eat_off, 1);
    // 2) 越界两向：低于基 / 高于基+计数。
    let low = eat_resolve_by_ordinal(&m, eat_off, 0);
    let high = eat_resolve_by_ordinal(&m, eat_off, 2);
    cs.add(
        "eat_ordinal_direct_and_bounds",
        hit == Some(0x1234_5678) && low.is_none() && high.is_none(),
        "",
    );
    // 3) 空表（NumberOfFunctions=0）如实拒——不把 0 当「全合法」。
    m[eat_off + EAT_OFF_COUNT..eat_off + EAT_OFF_COUNT + 4].copy_from_slice(&0u32.to_le_bytes());
    cs.add(
        "eat_ordinal_empty_table_none",
        eat_resolve_by_ordinal(&m, eat_off, 1).is_none(),
        "",
    );
    cs
}
