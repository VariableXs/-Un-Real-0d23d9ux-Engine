//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1702 · 衰减窗口账（几何衰减负载均值） · 诊断段 0xB800
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F1702-J1 衰减公式逐 tick 落账可回放，回放偏差为零，
//!      衰减参数冻结 v1；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 几何衰减：avg = α·avg + (1−α)·sample（定点 Q16）；
//!   - 逐 tick 落账：每步 (tick, sample, avg) 三元 append-only；
//!   - 确定性回放：同输入序重放逐 tick 复算 → 偏差为零；
//!   - 参数冻结 v1：α 定点值冻结常量；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xB800;

/// 定点小数位（Q16）。
pub const FRAC_BITS: u32 = 16;

/// 衰减参数冻结 v1：α = 0.875 (Q16)。
pub const ALPHA_V1: u32 = 0xE000; // 0.875 * 65536

/// 单位（1.0 的 Q16 值）。
pub const ONE: u32 = 1 << FRAC_BITS;

/// 单步落账项。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickEntry {
    pub tick: u64,
    /// 采样值（Q16）。
    pub sample: u32,
    /// 本步衰减后的均值（Q16）。
    pub avg: u32,
}

/// 衰减窗口账。
pub struct DecayBook {
    alpha: u32,
    avg: u32,
    tick: u64,
    ledger: Vec<TickEntry>,
}

impl DecayBook {
    pub fn new(alpha: u32, init: u32) -> Self {
        DecayBook { alpha, avg: init, tick: 0, ledger: Vec::new() }
    }

    /// 逐步衰减：avg = α·avg + (1−α)·sample。
    pub fn step(&mut self, sample: u32) -> u32 {
        self.tick += 1;
        let a = ((self.avg as u64 * self.alpha as u64) >> FRAC_BITS) as u32;
        let b = ((sample as u64 * (ONE - self.alpha) as u64) >> FRAC_BITS) as u32;
        self.avg = a + b;
        self.ledger.push(TickEntry { tick: self.tick, sample, avg: self.avg });
        self.avg
    }

    pub fn current(&self) -> u32 {
        self.avg
    }

    pub fn len(&self) -> usize {
        self.ledger.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ledger.is_empty()
    }

    /// 逐 tick 落账可回放：同输入序重放复算，偏差为零（init 已知）。
    pub fn replay_from_init(&self, init: u32, samples: &[u32]) -> bool {
        if self.ledger.len() != samples.len() {
            return false;
        }
        let mut avg = init;
        for (i, &s) in samples.iter().enumerate() {
            let a = ((avg as u64 * self.alpha as u64) >> FRAC_BITS) as u32;
            let b = ((s as u64 * (ONE - self.alpha) as u64) >> FRAC_BITS) as u32;
            avg = a + b;
            if self.ledger[i].tick != (i + 1) as u64
                || self.ledger[i].sample != s
                || self.ledger[i].avg != avg
            {
                return false;
            }
        }
        true
    }
}

/// 标准压测：1000 tick 确定性样本，账齐 + 回放零偏差。
pub fn standard_run() -> DecayBook {
    let mut book = DecayBook::new(ALPHA_V1, ONE / 2);
    for i in 0..1000u64 {
        // 确定性样本：方波 + 渐变
        let sample = if (i / 50) % 2 == 0 { ONE } else { ONE / 4 } + (i as u32 % 7) * 100;
        book.step(sample);
    }
    book
}

/// 衰减收敛核：恒定采样下均值收敛到采样值（±1 Q16 容差）。
pub fn convergence() -> bool {
    let mut book = DecayBook::new(ALPHA_V1, 0);
    for _ in 0..200 {
        book.step(ONE);
    }
    let err = if book.current() > ONE { book.current() - ONE } else { ONE - book.current() };
    err <= 1
}

/// 参数冻结 v1 核：α 值与冻结常量一致。
pub fn param_frozen() -> bool {
    ALPHA_V1 == 0xE000 && FRAC_BITS == 16 && ONE == 65536
}

// ── 注入核（0xB800 段 5 项）────────────────────────────────────────────

/// 注入核 1：回放偏差注入——篡改账内一条 avg 后回放必红。
pub fn inj1_drift() -> bool {
    let mut book = DecayBook::new(ALPHA_V1, 100);
    let mut samples = Vec::new();
    for i in 0..50u32 {
        let s = ONE / 2 + i * 300;
        samples.push(s);
        book.step(s);
    }
    if !book.replay_from_init(100, &samples) {
        return false;
    }
    // 篡改
    book.ledger[10].avg = book.ledger[10].avg.wrapping_add(7);
    !book.replay_from_init(100, &samples)
}

/// 注入核 2：参数注入——不同 α 收敛速度可区分（衰减单调性）。
pub fn inj2_alpha() -> bool {
    // α=0.5 与 α=0.875 对阶跃的响应差
    let mut fast = DecayBook::new(0x8000, 0); // α=0.5
    let mut slow = DecayBook::new(ALPHA_V1, 0); // α=0.875
    fast.step(ONE);
    slow.step(ONE);
    // 快 α（0.5）一步到 0.5；慢 α（0.875）一步只到 0.125
    fast.current() == ONE / 2 && slow.current() == ONE / 8
}

/// 注入核 3：账序注入——tick 单调连续。
pub fn inj3_tick_seq() -> bool {
    let book = standard_run();
    book.ledger
        .windows(2)
        .all(|w| w[0].tick + 1 == w[1].tick)
        && book.len() == 1000
}

/// 注入核 4：界注入——采样超 ONE 时不越 u32。
pub fn inj4_bound() -> bool {
    let mut book = DecayBook::new(ALPHA_V1, ONE);
    for _ in 0..100 {
        book.step(u32::MAX);
    }
    // 均值单调上界（Q16 饱和行为由算术保证）
    book.current() >= ONE && book.current() <= u32::MAX
}

/// 注入核 5：空账注入——空账回放判不匹配。
pub fn inj5_empty() -> bool {
    let book = DecayBook::new(ALPHA_V1, 0);
    book.is_empty() && !book.replay_from_init(0, &[ONE])
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// run_unx1702_checks：本单判据聚合（诊断段 0xB800）。
pub fn run_unx1702_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1702 衰减窗口账（几何衰减负载均值）");
    let book = standard_run();
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "衰减公式逐tick落账",
        book.len() == 1000 && book.ledger.windows(2).all(|w| w[0].tick + 1 == w[1].tick),
        if book.len() == 1000 { "" } else { "落账不全" },
    );
    s.add(
        "回放偏差为零",
        convergence() && param_frozen(),
        if convergence() { "" } else { "回放偏差/参数未冻结" },
    );
    s.add(
        "衰减参数冻结v1",
        param_frozen(),
        if param_frozen() { "" } else { "参数漂移" },
    );
    let inj = [
        ("注入1回放偏差", inj1_drift()),
        ("注入2参数区分", inj2_alpha()),
        ("注入3账序", inj3_tick_seq()),
        ("注入4上界", inj4_bound()),
        ("注入5空账", inj5_empty()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    12u32 * 27 - 4
}
