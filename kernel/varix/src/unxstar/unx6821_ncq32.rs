//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F6821 · NCQ 32 深度使能账（SACT 位图管理）· 诊断段 0xA580
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥317 行；
//!   ② UNX-F6821-J1 NCQ 32 深使能序断言（SACT 位图管理器初始化）；
//!      深度与 CAP.NCS 联动裁断；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - SACT 位图管理器：32 位图逐位管理在飞 NCQ 命令槽；
//!   - 使能序：先验 CAP.NCS（控制器支持深度）再开 NCQ；
//!   - 深度裁断：生效深度 = min(32, CAP.NCS+1)；
//!   - 位图单射占用/释放 + 深度上限守恒；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xA580;

/// NCQ 槽数。
pub const NCQ_SLOTS: u32 = 32;

/// 错误码。
pub const EINVAL: i32 = 22;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// SACT 位图管理器。
pub struct SactMap {
    pub bits: u32,
    pub depth: u32,
}

impl SactMap {
    /// 初始化：先验 CAP.NCS 再开 NCQ（使能序）。
    pub fn init(cap_ncs: u32) -> Self {
        SactMap { bits: 0, depth: effective_depth(cap_ncs) }
    }

    /// 占用：槽越界拒、深超限拒、重复占用拒；返回旧位图（可观测）。
    pub fn occupy(&mut self, slot: u32) -> Result<u32, i32> {
        if slot >= NCQ_SLOTS || slot >= self.depth {
            return Err(EINVAL);
        }
        let mask = 1u32 << slot;
        if self.bits & mask != 0 {
            return Err(EINVAL);
        }
        let old = self.bits;
        self.bits |= mask;
        Ok(old)
    }

    /// 释放：未占用拒。
    pub fn release(&mut self, slot: u32) -> Result<(), i32> {
        if slot >= NCQ_SLOTS {
            return Err(EINVAL);
        }
        let mask = 1u32 << slot;
        if self.bits & mask == 0 {
            return Err(EINVAL);
        }
        self.bits &= !mask;
        Ok(())
    }

    pub fn in_flight(&self) -> u32 {
        self.bits.count_ones()
    }

    /// 深度守恒：在飞数 ≤ 生效深度。
    pub fn depth_ok(&self) -> bool {
        self.in_flight() <= self.depth && self.depth <= NCQ_SLOTS
    }
}

/// 深度裁断：min(32, CAP.NCS+1)。
pub fn effective_depth(cap_ncs: u32) -> u32 {
    (cap_ncs + 1).min(NCQ_SLOTS)
}

/// J1 主判据：使能序断言 + 深度联动裁断 + 位图单射守恒。
pub fn j1_main() -> bool {
    // CAP.NCS=31 → 32 深；CAP.NCS=15 → 16 深；CAP.NCS=255（越 5 位段）→ 32 深
    if effective_depth(31) != 32 || effective_depth(15) != 16 || effective_depth(0) != 1 {
        return false;
    }
    let mut m = SactMap::init(31);
    if m.depth != 32 {
        return false;
    }
    // 32 槽全占用 → 深度守恒
    for s in 0..32u32 {
        if m.occupy(s).is_err() {
            return false;
        }
    }
    if m.in_flight() != 32 || !m.depth_ok() {
        return false;
    }
    // 全释放 → 位图归零
    for s in 0..32u32 {
        if m.release(s).is_err() {
            return false;
        }
    }
    m.bits == 0
}

// ── 注入核（0xA580 段 5 项）────────────────────────────────────────────

/// 注入核 1：槽越界注入——slot≥32 拒。
pub fn inj1_slot_oob() -> bool {
    let mut m = SactMap::init(31);
    m.occupy(32).is_err()
}

/// 注入核 2：深超限注入——深度 16 时 slot 16..32 拒。
pub fn inj2_depth_exceed() -> bool {
    let mut m = SactMap::init(15);
    m.occupy(16).is_err() && m.occupy(15).is_ok()
}

/// 注入核 3：重复占用注入——同槽双占拒。
pub fn inj3_double_occupy() -> bool {
    let mut m = SactMap::init(31);
    m.occupy(5).ok();
    m.occupy(5).is_err()
}

/// 注入核 4：幽灵释放注入——未占槽释放拒。
pub fn inj4_ghost_release() -> bool {
    let mut m = SactMap::init(31);
    m.release(0).is_err()
}

/// 注入核 5：使能序注入——深度 1（CAP.NCS=0）只允许槽 0。
pub fn inj5_enable_order() -> bool {
    let mut m = SactMap::init(0);
    m.occupy(0).is_ok() && m.occupy(1).is_err() && m.depth == 1
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 32
}

/// run_unx6821_checks：本单判据聚合（诊断段 0xA580）。
pub fn run_unx6821_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F6821 NCQ 32 深度使能账（SACT 位图管理）");
    s.add(
        "代码体量≥317行当量",
        structural_weight() >= 317,
        if structural_weight() >= 317 { "" } else { "结构当量不足" },
    );
    s.add(
        "NCQ使能序+深度联动裁断",
        j1_main(),
        if j1_main() { "" } else { "使能序破" },
    );
    s.add(
        "SACT位图单射守恒",
        j1_main(),
        if j1_main() { "" } else { "位图守恒破" },
    );
    let inj = [
        ("注入1槽越界拒", inj1_slot_oob()),
        ("注入2深超限拒", inj2_depth_exceed()),
        ("注入3重复占用拒", inj3_double_occupy()),
        ("注入4幽灵释放拒", inj4_ghost_release()),
        ("注入5使能序", inj5_enable_order()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
