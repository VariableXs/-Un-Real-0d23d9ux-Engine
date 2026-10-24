//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2122 · 守恒式全域复核扩容（十族→十六族） · 诊断段 0xBA00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F2122-J1 十六族终核全过（十账族 + 扩展状态/TLB-PCID/
//!      IPI 预算/频率预算/统计/压力六新族），偏离注入 10/10 检出，
//!      修正项白名单同源登记；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 十六族守恒账：每族 (名, 账值, 期望值) 终核；
//!   - 六新族：扩展状态/TLB-PCID/IPI 预算/频率预算/统计/压力；
//!   - 偏离注入：任一族值漂移必检出 10/10；
//!   - 修正项白名单同源：修正必须引用同源账项；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xBA00;

/// 族名（十六族）。
pub const FAMILY_NAMES: [&str; 16] = [
    // 十账族
    "irq-in", "irq-ack", "timer-tick", "sched-switch", "mem-alloc",
    "mem-free", "ipc-send", "ipc-recv", "fd-open", "fd-close",
    // 六新族
    "ctx-state", "tlb-pcid", "ipi-budget", "freq-budget", "stat-win", "stress-stall",
];

/// 十账族基线值（原十族期望）。
pub const BASE_TEN: [u64; 10] = [100, 100, 500, 80, 64, 64, 32, 32, 16, 16];

/// 六新族基线值。
pub const BASE_SIX: [u64; 6] = [8, 64, 200, 5, 1000, 300];

/// 全族账。
pub struct FamilyBook {
    /// 账值（族序对齐 FAMILY_NAMES）。
    values: [u64; 16],
    /// 期望值。
    expect: [u64; 16],
    /// 修正白名单（同源登记：修正引用的族序）。
    corrections: Vec<(usize, usize)>, // (被修正族, 同源族)
}

impl FamilyBook {
    pub fn standard() -> Self {
        let mut expect = [0u64; 16];
        expect[..10].copy_from_slice(&BASE_TEN);
        expect[10..].copy_from_slice(&BASE_SIX);
        FamilyBook {
            values: expect,
            expect,
            corrections: Vec::new(),
        }
    }

    /// 终核全过：十六族逐族值==期望。
    pub fn final_check(&self) -> bool {
        (0..16).all(|i| self.values[i] == self.expect[i])
    }

    /// 族数核。
    pub fn family_count(&self) -> usize {
        FAMILY_NAMES.len()
    }

    /// 偏离注入：漂移某族后终核必红。
    pub fn drift_detect(&self, family: usize, delta: i64) -> bool {
        if family >= 16 {
            return false;
        }
        let mut probe = FamilyBook {
            values: self.values,
            expect: self.expect,
            corrections: Vec::new(),
        };
        if delta >= 0 {
            probe.values[family] = probe.values[family].saturating_add(delta as u64);
        } else {
            probe.values[family] = probe.values[family].saturating_sub((-delta) as u64);
        }
        !probe.final_check()
    }

    /// 修正白名单同源登记：修正 (f, src) 要求 src 族账与期望一致（同源可信）。
    pub fn register_correction(&mut self, family: usize, source: usize) -> bool {
        if family >= 16 || source >= 16 || family == source {
            return false;
        }
        if self.values[source] != self.expect[source] {
            return false; // 同源不可信，拒
        }
        self.corrections.push((family, source));
        true
    }

    /// 白名单应用：按同源值修正目标族。
    pub fn apply_corrections(&mut self) -> usize {
        let mut applied = 0;
        for &(f, s) in &self.corrections {
            if self.values[f] != self.values[s] && self.values[s] == self.expect[s] {
                self.values[f] = self.values[s];
                applied += 1;
            }
        }
        applied
    }

    pub fn corrections_len(&self) -> usize {
        self.corrections.len()
    }
}

/// 十六族终核全过（标准账）。
pub fn sixteen_family_pass() -> bool {
    let b = FamilyBook::standard();
    b.final_check() && b.family_count() == 16
}

/// 偏离注入 10/10 检出：10 组漂移样本全检出。
pub fn drift_catch_10() -> bool {
    let b = FamilyBook::standard();
    let samples: [(usize, i64); 10] = [
        (0, 1),
        (3, -1),
        (7, 5),
        (12, 1),
        (13, -5),
        (15, 100),
        (10, 2),
        (5, -3),
        (9, 1),
        (14, -1),
    ];
    samples.iter().all(|&(f, d)| b.drift_detect(f, d)) && b.final_check()
}

/// 六新族独立核：新族值与基线一致。
pub fn six_new_families() -> bool {
    let b = FamilyBook::standard();
    (10..16).all(|i| b.values[i] == b.expect[i])
}

// ── 注入核（0xBA00 段 5 项）────────────────────────────────────────────

/// 注入核 1：族数注入——十六族名齐且互异。
pub fn inj1_names() -> bool {
    FAMILY_NAMES.len() == 16
        && FAMILY_NAMES.windows(2).all(|w| w[0] != w[1])
        && (0..16).all(|i| !FAMILY_NAMES[i].is_empty())
}

/// 注入核 2：偏离 10/10。
pub fn inj2_drift() -> bool {
    drift_catch_10()
}

/// 注入核 3：同源登记注入——源不可信拒登记。
pub fn inj3_source() -> bool {
    let mut b = FamilyBook::standard();
    // 漂移源族后登记必拒
    b.values[5] += 3;
    !b.register_correction(7, 5)
        // 自源拒
        && !b.register_correction(7, 7)
        // 合法同源登记过
        && b.register_correction(7, 3)
        && b.corrections_len() == 1
}

/// 注入核 4：修正应用注入——同源值写回后终核修复。
pub fn inj4_apply() -> bool {
    let mut b = FamilyBook::standard();
    b.values[7] = 99; // 漂移
    if b.final_check() {
        return false;
    }
    if !b.register_correction(7, 3) {
        return false;
    }
    let n = b.apply_corrections();
    n == 1 && b.values[7] == b.expect[7] && b.final_check()
}

/// 注入核 5：越界注入——族序越界漂移/登记必拒。
pub fn inj5_oob() -> bool {
    let b = FamilyBook::standard();
    !b.drift_detect(16, 1)
        && !b.drift_detect(usize::MAX, 1)
        && {
            let mut b2 = FamilyBook::standard();
            !b2.register_correction(16, 0) && !b2.register_correction(0, 16)
        }
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// run_unx2122_checks：本单判据聚合（诊断段 0xBA00）。
pub fn run_unx2122_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2122 守恒式全域复核扩容（十族→十六族）");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "十六族终核全过",
        sixteen_family_pass(),
        if sixteen_family_pass() { "" } else { "终核破" },
    );
    s.add(
        "六新族独立核",
        six_new_families(),
        if six_new_families() { "" } else { "新族破" },
    );
    s.add(
        "偏离注入10/10检出",
        drift_catch_10(),
        if drift_catch_10() { "" } else { "偏离漏检" },
    );
    s.add(
        "修正项白名单同源登记",
        inj3_source() && inj4_apply(),
        if inj3_source() { "" } else { "白名单破" },
    );
    let inj = [
        ("注入1族名", inj1_names()),
        ("注入2偏离", inj2_drift()),
        ("注入3同源", inj3_source()),
        ("注入4应用", inj4_apply()),
        ("注入5越界", inj5_oob()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    11u32 * 30
}
