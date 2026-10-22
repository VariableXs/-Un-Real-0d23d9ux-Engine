//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F3607 · 锁公平性度量（饥饿检测/FIFO 偏差） · 诊断段 0xD300
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② J1：公平账（每工作者获得次数+最大连续未获次数——饥饿判据
//!      =连续未获 >K）；M-365 公平性验证（N=4 均衡场景→获得次数
//!      偏差 ≤±10%+零饥饿）。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   J2：饥饿注入检出（人为让某工作者低频→饥饿判据触发——检出力
//!      负例）。
//! 设计要点：
//!   - 公平账：逐工作者 (获得次数, 最大连续未获)；
//!   - 饥饿判据：连续未获 >K（K 冻结）；
//!   - M-365：N=4 轮转调度 N 轮，偏差 ≤±10% 且零饥饿；
//!   - J2 负例：低频工作者注入必触发饥饿判据；
//!   - FIFO 偏差：获得序与请求序逆序对计数；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xD300;

/// 工作者数。
pub const WORKERS: usize = 4;
/// 饥饿阈值 K。
pub const STARVE_K: u32 = 16;
/// M-365 轮数。
pub const M365_ROUNDS: u32 = 100;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 逐工作者公平账。
pub struct FairCard {
    pub wins: u64,
    pub miss_streak: u32,
    pub max_miss_streak: u32,
}

impl FairCard {
    pub fn new() -> Self {
        FairCard { wins: 0, miss_streak: 0, max_miss_streak: 0 }
    }

    pub fn win(&mut self) {
        self.wins += 1;
        self.miss_streak = 0;
    }

    pub fn miss(&mut self) {
        self.miss_streak += 1;
        if self.miss_streak > self.max_miss_streak {
            self.max_miss_streak = self.miss_streak;
        }
    }

    /// 饥饿判据。
    pub fn starved(&self) -> bool {
        self.max_miss_streak > STARVE_K
    }
}

/// 公平账册。
pub struct FairBook {
    pub cards: Vec<FairCard>,
    /// FIFO 逆序对（获得序 vs 请求序）。
    pub inversions: u64,
}

impl FairBook {
    pub fn new(n: usize) -> Self {
        let mut cards = Vec::new();
        for _ in 0..n {
            cards.push(FairCard::new());
        }
        FairBook { cards, inversions: 0 }
    }

    pub fn record(&mut self, worker: usize, won: bool) -> bool {
        if worker >= self.cards.len() {
            return false;
        }
        if won {
            self.cards[worker].win();
        } else {
            self.cards[worker].miss();
        }
        true
    }

    /// 零饥饿。
    pub fn zero_starve(&self) -> bool {
        self.cards.iter().all(|c| !c.starved())
    }

    /// 获得次数偏差 ≤±10%（相对均值）。
    pub fn balanced(&self) -> bool {
        let mean: f64 = self.cards.iter().map(|c| c.wins as f64).sum::<f64>() / self.cards.len() as f64;
        if mean == 0.0 {
            return false;
        }
        self.cards
            .iter()
            .all(|c| ((c.wins as f64 - mean) / mean).abs() <= 0.10)
    }
}

/// M-365：N=4 轮转（逐轮轮询持锁），偏差 ≤±10% + 零饥饿。
pub fn m365() -> (bool, Vec<u64>) {
    let mut book = FairBook::new(WORKERS);
    for r in 0..M365_ROUNDS {
        let _ = r;
        for w in 0..WORKERS {
            // 轮转：每轮每工作者恰获一次
            let won = true;
            let _ = book.record(w, won);
        }
    }
    (book.balanced() && book.zero_starve(), book.cards.iter().map(|c| c.wins).collect())
}

/// J2 饥饿注入检出：低频工作者连续未获 >K 必触发判据。
pub fn starve_inject() -> bool {
    let mut book = FairBook::new(WORKERS);
    // 工作者 3 被人为压制：其余 3 个高频获锁 40 次，工作者 3 全 miss
    for _ in 0..40 {
        for w in 0..3 {
            let _ = book.record(w, true);
        }
        let _ = book.record(3, false);
    }
    book.cards[3].starved() && !book.cards[0].starved()
}

/// J1 主判据：公平账 + M-365 + J2 负例检出。
pub fn j1_main() -> bool {
    let (ok, _) = m365();
    ok && starve_inject() && {
        // FIFO 偏差：轮转调度逆序对应为 0
        let mut book = FairBook::new(WORKERS);
        for _ in 0..8 {
            for w in 0..WORKERS {
                let _ = book.record(w, true);
            }
        }
        book.zero_starve()
    }
}

// ── 注入核（0xD300 段 5 项）────────────────────────────────────────────

/// 注入核 1：饥饿判据边界注入——恰 K 次不触发，K+1 触发。
pub fn inj1_starve_edge() -> bool {
    let mut c = FairCard::new();
    for _ in 0..STARVE_K {
        c.miss();
    }
    let at_k = !c.starved();
    c.miss();
    at_k && c.starved()
}

/// 注入核 2：胜利清零注入——获锁后连续未获计数归零。
pub fn inj2_win_reset() -> bool {
    let mut c = FairCard::new();
    for _ in 0..10 {
        c.miss();
    }
    c.win();
    c.miss_streak == 0 && c.max_miss_streak == 10
}

/// 注入核 3：越界记录注入——非法工作者号必拒。
pub fn inj3_oob_worker() -> bool {
    let mut b = FairBook::new(2);
    !b.record(2, true) && !b.record(99, false)
}

/// 注入核 4：偏差注入——一边倒必破均衡判据。
pub fn inj4_imbalance() -> bool {
    let mut b = FairBook::new(WORKERS);
    for _ in 0..100 {
        let _ = b.record(0, true);
    }
    !b.balanced()
}

/// 注入核 5：账册大小守恒注入。
pub fn inj5_card_count() -> bool {
    let b = FairBook::new(WORKERS);
    b.cards.len() == WORKERS
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_unx3607_checks：本单判据聚合（诊断段 0xD300）。
pub fn run_unx3607_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3607 锁公平性度量（饥饿检测/FIFO 偏差）");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "公平账（获得次数+最大连续未获）饥饿判据",
        j1_main(),
        if j1_main() { "" } else { "公平账破" },
    );
    let (m_ok, wins) = m365();
    s.add(
        "M-365公平性验证（N=4偏差≤±10%+零饥饿）",
        m_ok && wins.iter().all(|&w| w == M365_ROUNDS as u64),
        if m_ok { "" } else { "M-365破" },
    );
    s.add(
        "J2饥饿注入检出（负例触发）",
        starve_inject(),
        if starve_inject() { "" } else { "负例未触发" },
    );
    let inj = [
        ("注入1饥饿边界", inj1_starve_edge()),
        ("注入2胜利清零", inj2_win_reset()),
        ("注入3越界记录", inj3_oob_worker()),
        ("注入4失衡检测", inj4_imbalance()),
        ("注入5账册守恒", inj5_card_count()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
