
// ---------------------------------------------------------------------------
// F020 · 深化批次八：崩溃转储模块表去重合并（同一模块被多线程栈重复引用
// 时去重；基址重叠 = 同一模块的不同表述，保首个、合并引用计数——列表
// 诚实可读，不是流水账）。
// ---------------------------------------------------------------------------

/// 模块条目（与批次五/六的 addr 区间语义一致：base..base+size）。
#[derive(Clone, Copy)]
pub struct CrashMod {
    pub base: u64,
    pub size: u32,
    pub refs: u32,
}

/// 去重合并：base+size 区间完全相同 → 合并 refs（取和）；其余保留。
/// 返回去重后条目数。输入不改（审计面不可变——调用方拿结果另存）。
pub fn crash_mods_dedup(mods: &[CrashMod], out: &mut alloc::vec::Vec<CrashMod>) -> usize {
    out.clear();
    for m in mods {
        match out.iter_mut().find(|o| o.base == m.base && o.size == m.size) {
            Some(o) => o.refs += m.refs,
            None => out.push(*m),
        }
    }
    out.len()
}

/// F020 深化批次八自检。
fn run_excface_deep7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F020-excface-deep7");
    let src = [
        CrashMod { base: 0x7FF6_1000_0000, size: 0x1000, refs: 3 },
        CrashMod { base: 0x7FF8_2000_0000, size: 0x8000, refs: 1 },
        CrashMod { base: 0x7FF6_1000_0000, size: 0x1000, refs: 2 },
        CrashMod { base: 0x7FF6_1000_0000, size: 0x1000, refs: 1 },
    ];
    let mut dst = alloc::vec::Vec::new();
    // 1) 同区间三引用合并 → refs 6；其余保留；输入不变（审计不可变纪律）。
    let n = crash_mods_dedup(&src, &mut dst);
    cs.add(
        "crash_mods_dedup_merge_refs",
        n == 2
            && dst[0].refs == 6
            && dst[1].refs == 1
            && src[0].refs == 3
            && src[2].refs == 2,
        "",
    );
    // 2) 相邻但不同区间不去重（0x1000 与 0x1001 是两个模块表述——不猜）。
    let near = [
        CrashMod { base: 0x1000, size: 0x10, refs: 1 },
        CrashMod { base: 0x1000, size: 0x11, refs: 1 },
    ];
    let n2 = crash_mods_dedup(&near, &mut dst);
    cs.add(
        "crash_mods_adjacent_kept",
        n2 == 2,
        "",
    );
    // 3) 空表 → 0（不产幽灵条目）。
    let n3 = crash_mods_dedup(&[], &mut dst);
    cs.add(
        "crash_mods_empty",
        n3 == 0 && dst.is_empty(),
        "",
    );
    cs
}
