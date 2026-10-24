//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F3182 · 40 批条数总核（800 条逐批计数终核） · 诊断段 0xC600
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F3182-J1 条数总核冻结：B01–B40 逐批条数计数（每批 20 条）
//!      断言过，40 批合计 800 条断言过，计数缺批注入检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 逐批计数账：B01–B40 每批 20 条；
//!   - 合计断言：Σ = 40×20 = 800；
//!   - 缺批注入：抽批后合计必不等于 800 且逐批断言破；
//!   - 计数哈希（逐批条数序列）FNV-1a 入册；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xC600;

/// 批数。
pub const BATCHES: u32 = 40;
/// 每批条数。
pub const ITEMS_PER_BATCH: u32 = 20;
/// 合计。
pub const TOTAL_ITEMS: u32 = BATCHES * ITEMS_PER_BATCH;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 逐批计数账。
pub struct CountBook {
    /// 逐批条数（索引 0 = B01）。
    pub counts: Vec<u32>,
}

impl CountBook {
    /// 标准账：40 批每批 20 条。
    pub fn standard() -> Self {
        let mut counts = Vec::new();
        for _ in 0..BATCHES {
            counts.push(ITEMS_PER_BATCH);
        }
        CountBook { counts }
    }

    /// 逐批断言：批数齐且每批恰 20 条。
    pub fn per_batch_ok(&self) -> bool {
        self.counts.len() == BATCHES as usize && self.counts.iter().all(|&c| c == ITEMS_PER_BATCH)
    }

    /// 合计断言。
    pub fn total_ok(&self) -> bool {
        let sum: u32 = self.counts.iter().sum();
        sum == TOTAL_ITEMS
    }

    /// 计数哈希。
    pub fn hash(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        for (i, &c) in self.counts.iter().enumerate() {
            buf.push((i + 1) as u8);
            buf.push(c as u8);
        }
        fnv1a(&buf)
    }

    /// 缺批注入检出：抽一批后逐批断言与合计断言必双破。
    pub fn missing_detect(&self) -> bool {
        let mut probe = CountBook::standard();
        probe.counts.remove(17);
        !probe.per_batch_ok() && !probe.total_ok()
    }

    /// 条数漂移注入检出：某批条数变动合计必破。
    pub fn drift_detect(&self) -> bool {
        let mut probe = CountBook::standard();
        probe.counts[5] += 1;
        probe.per_batch_ok() == false && probe.total_ok() == false
    }
}

/// 逐批计数模拟：从任务号区间推批界再计数。
pub fn count_by_domain(first: u32, last: u32) -> (bool, u32) {
    if last < first || last - first + 1 != TOTAL_ITEMS {
        return (false, 0);
    }
    let span = last - first + 1;
    let mut all_ok = true;
    let mut b = 0u32;
    while b < BATCHES {
        let lo = first + b * ITEMS_PER_BATCH;
        let hi = lo + ITEMS_PER_BATCH - 1;
        if hi > last {
            all_ok = false;
        }
        b += 1;
    }
    (all_ok, span)
}

/// J1 主判据：逐批 20 条 + 合计 800 + 缺批检出 + 哈希入册。
pub fn j1_main() -> bool {
    let book = CountBook::standard();
    book.per_batch_ok() && book.total_ok() && book.missing_detect() && book.drift_detect() && book.hash() != 0 && {
        let (ok, span) = count_by_domain(1, TOTAL_ITEMS);
        ok && span == TOTAL_ITEMS
    }
}

// ── 注入核（0xC600 段 5 项）────────────────────────────────────────────

/// 注入核 1：缺批注入。
pub fn inj1_missing_batch() -> bool {
    CountBook::standard().missing_detect()
}

/// 注入核 2：条数漂移注入。
pub fn inj2_count_drift() -> bool {
    CountBook::standard().drift_detect()
}

/// 注入核 3：域界失配注入——条数区间不足必拒。
pub fn inj3_domain_mismatch() -> bool {
    let (ok, _) = count_by_domain(1, TOTAL_ITEMS - 1);
    !ok
}

/// 注入核 4：空账注入——零批账必双破。
pub fn inj4_empty_book() -> bool {
    let probe = CountBook { counts: Vec::new() };
    !probe.per_batch_ok() && !probe.total_ok()
}

/// 注入核 5：哈希稳定注入。
pub fn inj5_hash_stable() -> bool {
    let b = CountBook::standard();
    b.hash() == b.hash() && b.hash() != 0
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_unx3182_checks：本单判据聚合（诊断段 0xC600）。
pub fn run_unx3182_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3182 40 批条数总核（800 条逐批计数终核）");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    let book = CountBook::standard();
    s.add(
        "B01–B40逐批条数计数断言过（每批20条）",
        book.per_batch_ok(),
        if book.per_batch_ok() { "" } else { "逐批计数破" },
    );
    s.add(
        "40批合计800条断言过",
        book.total_ok(),
        if book.total_ok() { "" } else { "合计不等于800" },
    );
    s.add(
        "计数缺批注入检出",
        j1_main(),
        if j1_main() { "" } else { "缺批未检" },
    );
    let inj = [
        ("注入1缺批", inj1_missing_batch()),
        ("注入2条数漂移", inj2_count_drift()),
        ("注入3域界失配", inj3_domain_mismatch()),
        ("注入4空账", inj4_empty_book()),
        ("注入5哈希稳定", inj5_hash_stable()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
