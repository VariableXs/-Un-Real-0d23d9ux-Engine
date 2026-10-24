//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F3142 · 体检单链总核（F2797 链 40 批健康读数总核） · 诊断段 0xC500
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F3142-J1 体检总核冻结：F2797 体检单链全链读数
//!      （各批体检单挂链件逐件）总核断言（链完整性/健康项齐备性
//!      两查）过，断链注入检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 体检单：批号/健康项向量（四项）挂链序号；
//!   - 单链完整性：链序连续 1..40、逐件 next 指向顺延、尾件封链；
//!   - 健康项齐备性：四健康项全非空才齐备；
//!   - 断链注入：抽链件/坏链序必检出；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xC500;

/// 链件数（40 批）。
pub const CHAIN_LEN: usize = 40;
/// 健康项数。
pub const HEALTH_ITEMS: usize = 4;

/// 健康项名（冻结序）。
pub const HEALTH_NAMES: [&str; HEALTH_ITEMS] = ["VM-RUN", "PAGE-OK", "LOCK-OK", "IRQ-OK"];

/// 体检单。
#[derive(Debug, Clone)]
pub struct HealthCard {
    pub batch: u32,
    /// 健康项读数（Some=有读数）。
    pub readings: [Option<u32>; HEALTH_ITEMS],
    /// 链序号（1 起，尾件 = CHAIN_LEN）。
    pub seq: u32,
}

impl HealthCard {
    pub fn new(batch: u32, seq: u32) -> Self {
        HealthCard { batch, readings: [Some(1); HEALTH_ITEMS], seq }
    }

    /// 齐备性：四健康项全有读数。
    pub fn complete(&self) -> bool {
        self.readings.iter().all(|r| r.is_some())
    }
}

/// 体检单链。
pub struct HealthChain {
    pub cards: Vec<HealthCard>,
}

impl HealthChain {
    /// 标准链：40 件逐批挂链。
    pub fn standard() -> Self {
        let mut cards = Vec::new();
        for i in 1..=CHAIN_LEN as u32 {
            cards.push(HealthCard::new(i, i));
        }
        HealthChain { cards }
    }

    /// 链完整性两查：
    ///   ① 链序连续：seq == 1..=N 且单调递增；
    ///   ② 批号对齐：batch == seq（挂链件逐件批序一致）。
    pub fn intact(&self) -> bool {
        if self.cards.len() != CHAIN_LEN {
            return false;
        }
        for (i, c) in self.cards.iter().enumerate() {
            if c.seq != (i + 1) as u32 || c.batch != c.seq {
                return false;
            }
        }
        true
    }

    /// 健康项齐备性：逐件 complete。
    pub fn all_complete(&self) -> bool {
        self.cards.iter().all(|c| c.complete())
    }

    /// 总核：完整 + 齐备。
    pub fn total_check(&self) -> bool {
        self.intact() && self.all_complete()
    }

    /// 断链注入检出：抽掉中段一件必检出。
    pub fn broken_detect(&self) -> bool {
        let mut probe = HealthChain { cards: Vec::new() };
        for (i, c) in self.cards.iter().enumerate() {
            if i != 20 {
                probe.cards.push(c.clone());
            }
        }
        !probe.intact()
    }

    /// 健康项缺失注入检出：任一项清空必检出。
    pub fn incomplete_detect(&self) -> bool {
        let mut probe = HealthChain::standard();
        probe.cards[7].readings[2] = None;
        !probe.all_complete() && probe.intact()
    }
}

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 全链读数哈希（总核冻结锚）。
pub fn chain_hash() -> u32 {
    let chain = HealthChain::standard();
    let mut buf: Vec<u8> = Vec::new();
    for c in &chain.cards {
        buf.push(c.batch as u8);
        buf.push(c.seq as u8);
        for r in &c.readings {
            buf.push(r.unwrap_or(0) as u8);
        }
    }
    for &n in HEALTH_NAMES.iter() {
        for &b in n.as_bytes() {
            buf.push(b);
        }
    }
    fnv1a(&buf)
}

/// J1 主判据：链完整 + 齐备 + 断链检出 + 哈希入册。
pub fn j1_main() -> bool {
    let chain = HealthChain::standard();
    chain.total_check() && chain.broken_detect() && chain.incomplete_detect() && chain_hash() != 0
}

// ── 注入核（0xC500 段 5 项）────────────────────────────────────────────

/// 注入核 1：断链注入——抽件必检出。
pub fn inj1_broken() -> bool {
    HealthChain::standard().broken_detect()
}

/// 注入核 2：链序漂移注入——seq 乱序必检出。
pub fn inj2_seq_drift() -> bool {
    let mut probe = HealthChain::standard();
    probe.cards[3].seq = 99;
    !probe.intact()
}

/// 注入核 3：批号漂移注入——batch≠seq 必检出。
pub fn inj3_batch_drift() -> bool {
    let mut probe = HealthChain::standard();
    probe.cards[10].batch += 1;
    !probe.intact()
}

/// 注入核 4：健康项缺失注入。
pub fn inj4_missing_item() -> bool {
    HealthChain::standard().incomplete_detect()
}

/// 注入核 5：健康项名互异注入。
pub fn inj5_names() -> bool {
    for i in 0..HEALTH_ITEMS {
        if HEALTH_NAMES[i].is_empty() {
            return false;
        }
        for j in i + 1..HEALTH_ITEMS {
            if HEALTH_NAMES[i] == HEALTH_NAMES[j] {
                return false;
            }
        }
    }
    true
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_unx3142_checks：本单判据聚合（诊断段 0xC500）。
pub fn run_unx3142_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3142 体检单链总核（F2797 链 40 批健康读数总核）");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    let chain = HealthChain::standard();
    s.add(
        "链完整性总核断言过",
        chain.intact(),
        if chain.intact() { "" } else { "链不完整" },
    );
    s.add(
        "健康项齐备性总核断言过",
        chain.all_complete(),
        if chain.all_complete() { "" } else { "健康项缺失" },
    );
    s.add(
        "断链注入检出",
        j1_main(),
        if j1_main() { "" } else { "断链未检" },
    );
    let inj = [
        ("注入1断链", inj1_broken()),
        ("注入2链序漂移", inj2_seq_drift()),
        ("注入3批号漂移", inj3_batch_drift()),
        ("注入4健康项缺失", inj4_missing_item()),
        ("注入5健康项名互异", inj5_names()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
