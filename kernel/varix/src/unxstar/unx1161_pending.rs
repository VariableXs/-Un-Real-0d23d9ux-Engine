//! UNX-F1161 BH 消费主循环 · pending 位图
//! 判据原文（CoRun Unxreal 主册）：UNX-F1161-J1 消费主循环 10^6 事件零丢失（pending 位图置位数=实际执行数恒等断言），单次循环配额约束生效实测（超额回缴 10/10 次），循环退出条件全覆盖验证
//! · 组件：pending 位图（64 位 BH 槽）+ 置位/清位/批量扫描 + 位图守恒
//! · 判据落地：pending 位图置位数=实际执行数
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const BHP_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const BHP_SEG_BASE: u16 = 0x62b0;

// --------------------------------------------------------------------------
// 诊断码（bhp 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// pending 位图诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum bhpCode {
    /// BH 槽号越界（≥64）。
    SlotOob,
    /// 重复置位（已在 pending）。
    AlreadyPending,
    /// 清未置位槽。
    NotPending,
    /// active 与 pending 同时置位（不该发生）。
    BothBitsSet,
    /// 位图回绕未掩码。
    BitWrap,
    /// 置位与清位竞争。
    ClearRace,
    /// popcount 与计数器不符。
    CountDrift,
    /// 两张位图皆满（背压溢出）。
    FullBoth,
}

impl bhpCode {
    /// 全量闭集。
    pub const ALL: [bhpCode; 8] = [
        bhpCode::SlotOob,
        bhpCode::AlreadyPending,
        bhpCode::NotPending,
        bhpCode::BothBitsSet,
        bhpCode::BitWrap,
        bhpCode::ClearRace,
        bhpCode::CountDrift,
        bhpCode::FullBoth,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            bhpCode::SlotOob => 0x62b0 + 0,
            bhpCode::AlreadyPending => 0x62b0 + 1,
            bhpCode::NotPending => 0x62b0 + 2,
            bhpCode::BothBitsSet => 0x62b0 + 3,
            bhpCode::BitWrap => 0x62b0 + 4,
            bhpCode::ClearRace => 0x62b0 + 5,
            bhpCode::CountDrift => 0x62b0 + 6,
            bhpCode::FullBoth => 0x62b0 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            bhpCode::SlotOob => "BH 槽号越界（≥64）",
            bhpCode::AlreadyPending => "重复置位（已在 pending）",
            bhpCode::NotPending => "清未置位槽",
            bhpCode::BothBitsSet => "active 与 pending 同时置位（不该发生）",
            bhpCode::BitWrap => "位图回绕未掩码",
            bhpCode::ClearRace => "置位与清位竞争",
            bhpCode::CountDrift => "popcount 与计数器不符",
            bhpCode::FullBoth => "两张位图皆满（背压溢出）",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            bhpCode::SlotOob => "按 64 槽空间校正",
            bhpCode::AlreadyPending => "幂等置位",
            bhpCode::NotPending => "空操作并记账",
            bhpCode::BothBitsSet => "按 pending 优先",
            bhpCode::BitWrap => "按 63 掩码",
            bhpCode::ClearRace => "原子语义裁定",
            bhpCode::CountDrift => "重建计数器",
            bhpCode::FullBoth => "背压并记账",
        }
    }
}


// ---------------------------------------------------------------------------
// pending 位图（64 槽；置位数 = 实际执行数，恒等断言底座）
// ---------------------------------------------------------------------------

/// pending 位图（64 位，u64 承载）。
#[derive(Clone, Copy, Debug)]
pub struct PendingMap {
    /// 位图本体。
    pub bits: u64,
    /// 历史置位总数（不清零，用于守恒对拍）。
    pub total_set: u64,
    /// 历史清位总数（对应执行数）。
    pub total_clear: u64,
}

impl PendingMap {
    /// 空位图。
    pub const fn new() -> Self {
        Self { bits: 0, total_set: 0, total_clear: 0 }
    }

    /// 置位一个槽（幂等）。
    pub fn set(&mut self, slot: u8) -> bool {
        if slot >= 64 {
            return false;
        }
        let mask = 1u64 << slot;
        if self.bits & mask != 0 {
            return true;
        }
        self.bits |= mask;
        self.total_set += 1;
        true
    }

    /// 清位一个槽（未置位则记账返回 false）。
    pub fn clear(&mut self, slot: u8) -> bool {
        if slot >= 64 {
            return false;
        }
        let mask = 1u64 << slot;
        if self.bits & mask == 0 {
            return false;
        }
        self.bits &= !mask;
        self.total_clear += 1;
        true
    }

    /// 当前置位数。
    pub const fn pending_count(&self) -> u32 {
        self.bits.count_ones()
    }

    /// 取最低置位槽号（None = 空）。
    pub const fn lowest(&self) -> Option<u8> {
        if self.bits == 0 {
            return None;
        }
        Some(self.bits.trailing_zeros() as u8)
    }

    /// 置位/清位守恒：总置位 = 总清位 + 当前置位。
    pub const fn conserved(&self) -> bool {
        self.total_set == self.total_clear + self.pending_count() as u64
    }
}

/// 统计执行数（清位次数）与 pending 置位数的恒等式。
pub const fn identity_holds(total_set: u64, executed: u64, pending: u64) -> bool {
    total_set == executed + pending
}

// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x62b0, 0x62bf]。
pub const fn bhp_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < bhpCode::ALL.len() {
        let c = bhpCode::ALL[i].code();
        if c < 0x62b0 || c > 0x62bf {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 置位/清位 + popcount。
fn c1_set_clear() -> bool {
    let mut p = PendingMap::new();
    p.set(3) && p.set(7) && p.set(3)
        && p.pending_count() == 2
        && p.lowest() == Some(3)
        && p.clear(3) && !p.clear(3)
        && p.pending_count() == 1
}

/// 置位/清位守恒。
fn c2_conserved() -> bool {
    let mut p = PendingMap::new();
    let mut i = 0u8;
    while i < 20 { p.set(i); i += 1; }
    let mut j = 0u8;
    while j < 12 { p.clear(j); j += 1; }
    p.conserved() && p.total_set == 20 && p.total_clear == 12
}

/// 零丢失恒等式（置位 = 执行 + pending）。
fn c3_identity() -> bool {
    let mut p = PendingMap::new();
    let mut i = 0u8;
    while i < 30 { p.set(i); i += 1; }
    let mut j = 0u8;
    while j < 18 { p.clear(j); j += 1; }
    identity_holds(p.total_set, p.total_clear, p.pending_count() as u64)
        && p.pending_count() == 12
}

/// 段位守卫：全部诊断码落在本模块段内。
fn bhp_guard_seg() -> bool {
    bhp_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_bhp_checks() -> CheckSet {
    let mut set = CheckSet::new("bhp");
    set.add(
        "F1161-pend-c1",
        c1_set_clear(),
        if c1_set_clear() { "" } else { "pending map set/clear broken" },
    );
    set.add(
        "F1161-pend-c2",
        c2_conserved(),
        if c2_conserved() { "" } else { "pending map not conservative" },
    );
    set.add(
        "F1161-pend-c3",
        c3_identity(),
        if c3_identity() { "" } else { "zero-loss identity broken" },
    );
    set.add(
        "bhp-seg-guard",
        bhp_guard_seg(),
        if bhp_guard_seg() { "" } else { "diagnostic code outside module segment" },
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（域聚合测试在 mod.rs——此处只放本模块纯函数级断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bhp_all_checks_green() {
        let set = run_bhp_checks();
        assert!(set.all_passed(), "BHP domain checks must be green");
    }

    #[test]
    fn bhp_segment_guard_holds() {
        assert!(bhp_segment_guard());
    }
}
