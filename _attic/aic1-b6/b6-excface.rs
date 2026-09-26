
// ---------------------------------------------------------------------------
// F020 · 深化批次六：dump 模块表按基址排序（CDB 视图惯例——地址定位前提）
//
// 主册依据（G-A-20【数据与存储】）：「minidump 含已加载模块表（含校验和）」
// ——CDB 导出前按基址升序排序（地址区间查找的惯例形态——开发者按崩溃地址
// 找模块的前提）。
// ---------------------------------------------------------------------------

/// 模块表按基址升序排序（就地——dump 面既有 modules 数组；插入排序零堆）。
pub fn sort_modules_by_base(modules: &mut [Option<DumpModule>]) {
    for i in 1..modules.len() {
        let key = match modules[i] {
            Some(m) => m,
            None => continue,
        };
        let mut j = i;
        while j > 0 {
            match modules[j - 1] {
                Some(prev) if prev.base > key.base => {
                    modules[j] = modules[j - 1];
                    j -= 1;
                }
                _ => break,
            }
        }
        modules[j] = Some(key);
    }
}

/// 按崩溃地址定位模块（基址 ≤ addr < base+size）——CDB `!analyze` 的定位核。
pub fn find_module_for_addr(modules: &[Option<DumpModule>], addr: u64) -> Option<usize> {
    modules
        .iter()
        .position(|m| matches!(m, Some(mm) if addr >= mm.base && addr < mm.base + mm.size as u64))
}

/// F020 深化批次六自检。
pub fn run_excface_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F020-excface-deep5");
    // 1) 排序：乱序三模块按基址升序（插入排序稳定——同名基址保持相对序）。
    let mut mods: [Option<DumpModule>; 4] = [
        Some(DumpModule { base: 0x7FF6_2000_0000, size: 0x1000, checksum: 2 }),
        Some(DumpModule { base: 0x7FF6_1000_0000, size: 0x1000, checksum: 1 }),
        Some(DumpModule { base: 0x7FF6_3000_0000, size: 0x1000, checksum: 3 }),
        None,
    ];
    sort_modules_by_base(&mut mods);
    cs.add(
        "modules_sorted_by_base",
        matches!(mods[0], Some(m) if m.checksum == 1)
            && matches!(mods[1], Some(m) if m.checksum == 2)
            && matches!(mods[2], Some(m) if m.checksum == 3),
        "",
    );
    // 2) 崩溃地址定位：落在第二模块区间 → idx 1；区间外如实 None。
    let hit = find_module_for_addr(&mods, 0x7FF6_1000_0800);
    let miss = find_module_for_addr(&mods, 0x7FF6_9000_0000);
    cs.add(
        "module_addr_lookup",
        hit == Some(1) && miss.is_none(),
        "",
    );
    // 3) 排序后 None 槽位不被打乱（模块数保真——排序不增不减）。
    cs.add("modules_none_slot_preserved", mods[3].is_none(), "");
    cs
}
