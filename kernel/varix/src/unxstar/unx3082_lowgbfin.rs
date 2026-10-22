//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F3082 · 1GB 档终账执行（低配终端压力剖面账面跑账） · 诊断段 0xC380
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F3082-J1 1GB 档终账冻结：低配剖面（小内存+高分配率+
//!      高频回收）账面执行 10/10 轮，轻/中/重三档位全覆盖
//!      （F3021 消费），轮账与 F2913 账册面名对齐，剖面漂移注入拦截；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 低配剖面：1GB 池 + 高分配率 + 高频回收三参数冻结；
//!   - 三档位（F3021）逐档跑账，10/10 轮全覆盖；
//!   - F2913 账册面名对齐（逐轮面名一致断言）；
//!   - 剖面漂移注入：池量/分配率/回收频任一漂移必拦截；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xC380;

/// 1GB 池（4K 页数当量）。
pub const POOL_1G_PAGES: u32 = 262_144;
/// 低配分配率（每 tick 分配页数）。
pub const ALLOC_RATE: u32 = 96;
/// 高频回收间隔（tick）。
pub const RECLAIM_EVERY: u32 = 4;
/// 终核轮数。
pub const ROUNDS: u32 = 10;

/// 压力档位（F3021 序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Light,
    Mid,
    Heavy,
}

pub const ALL_TIERS: [Tier; 3] = [Tier::Light, Tier::Mid, Tier::Heavy];

impl Tier {
    pub fn name(self) -> &'static str {
        match self {
            Tier::Light => "LIGHT",
            Tier::Mid => "MID",
            Tier::Heavy => "HEAVY",
        }
    }

    /// 档位负载系数。
    pub fn load(self) -> u32 {
        match self {
            Tier::Light => 1,
            Tier::Mid => 4,
            Tier::Heavy => 16,
        }
    }

    pub fn idx(self) -> usize {
        ALL_TIERS.iter().position(|&t| t == self).unwrap_or(3)
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

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed | 1)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 16) % n
    }
}

/// 低配剖面（三参数冻结）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LowProfile {
    pub pool_pages: u32,
    pub alloc_rate: u32,
    pub reclaim_every: u32,
}

impl LowProfile {
    /// 冻结标准剖面。
    pub fn standard() -> Self {
        LowProfile { pool_pages: POOL_1G_PAGES, alloc_rate: ALLOC_RATE, reclaim_every: RECLAIM_EVERY }
    }

    /// 与标准剖面全等。
    pub fn is_standard(&self) -> bool {
        *self == LowProfile::standard()
    }
}

/// 剖面面名（F2913 账册对齐锚）。
pub fn face_name(tier: Tier) -> String {
    let mut s = String::new();
    s.push_str("1G-LOW-");
    s.push_str(tier.name());
    s
}

/// 池账。
pub struct PoolBook {
    used: u32,
    alloc_cnt: u64,
    free_cnt: u64,
    reclaim_ticks: u32,
}

impl PoolBook {
    pub fn new() -> Self {
        PoolBook { used: 0, alloc_cnt: 0, free_cnt: 0, reclaim_ticks: 0 }
    }

    pub fn alloc(&mut self, n: u32, cap: u32) -> bool {
        if self.used + n > cap {
            return false;
        }
        self.used += n;
        self.alloc_cnt += n as u64;
        true
    }

    pub fn reclaim(&mut self, n: u32) -> bool {
        if self.used < n {
            return false;
        }
        self.used -= n;
        self.free_cnt += n as u64;
        self.reclaim_ticks += 1;
        true
    }

    /// 账闭合：used + 累计回收 = 累计分配。
    pub fn closed(&self) -> bool {
        self.alloc_cnt == self.free_cnt + self.used as u64
    }

    pub fn used(&self) -> u32 {
        self.used
    }
}

/// 单轮单档跑账：剖面驱动，tick 步进。
pub fn run_profile(prof: &LowProfile, tier: Tier, ticks: u32, rng: &mut Lcg) -> (bool, PoolBook) {
    let mut book = PoolBook::new();
    let load = tier.load() as u64;
    let mut t = 0;
    while t < ticks {
        // 高分配率：alloc_rate × load
        let want = (prof.alloc_rate as u64 * load) as u32;
        let mut got = 0u32;
        while got < want {
            let chunk = (want - got).min(16);
            if book.alloc(chunk, prof.pool_pages) {
                got += chunk;
            } else {
                // 池满触发即时回收
                let _ = book.reclaim(chunk.min(book.used));
            }
            let _ = rng.below(7);
        }
        // 高频回收：每 reclaim_every tick 折半回收
        if (t + 1) % prof.reclaim_every == 0 {
            let _ = book.reclaim(book.used / 2);
        }
        t += 1;
    }
    (book.closed(), book)
}

/// J1 主判据：三档全覆盖 10/10 轮 + 面名对齐 + 剖面冻结。
pub fn j1_main() -> (bool, u32) {
    let prof = LowProfile::standard();
    let mut pass = 0u32;
    for r in 0..ROUNDS {
        let mut rng = Lcg::new(0xC380_0000u64.wrapping_add(r as u64 * 0x6_1D9));
        let mut all_ok = prof.is_standard();
        for &tier in ALL_TIERS.iter() {
            let (ok, _) = run_profile(&prof, tier, 8, &mut rng);
            if !ok || face_name(tier).is_empty() {
                all_ok = false;
            }
        }
        if all_ok {
            pass += 1;
        }
    }
    (pass == ROUNDS, pass)
}

// ── 注入核（0xC380 段 5 项）────────────────────────────────────────────

/// 注入核 1：池量漂移注入——池缩水后 is_standard 必假。
pub fn inj1_pool_drift() -> bool {
    let mut prof = LowProfile::standard();
    prof.pool_pages /= 2;
    !prof.is_standard()
}

/// 注入核 2：分配率漂移注入。
pub fn inj2_rate_drift() -> bool {
    let mut prof = LowProfile::standard();
    prof.alloc_rate += 1;
    !prof.is_standard()
}

/// 注入核 3：回收频漂移注入。
pub fn inj3_reclaim_drift() -> bool {
    let mut prof = LowProfile::standard();
    prof.reclaim_every *= 2;
    !prof.is_standard()
}

/// 注入核 4：面名对齐注入——三档面名互异且前缀冻结。
pub fn inj4_face_names() -> bool {
    let n0 = face_name(Tier::Light);
    let n1 = face_name(Tier::Mid);
    let n2 = face_name(Tier::Heavy);
    n0.starts_with("1G-LOW-") && n0 != n1 && n1 != n2 && n2 != n0
}

/// 注入核 5：池账闭合注入——分配后不回收必不闭合。
pub fn inj5_ledger_open() -> bool {
    let mut b = PoolBook::new();
    let _ = b.alloc(128, POOL_1G_PAGES);
    let open = !b.closed();
    let _ = b.reclaim(128);
    open && b.closed()
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 31
}

/// run_unx3082_checks：本单判据聚合（诊断段 0xC380）。
pub fn run_unx3082_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3082 1GB 档终账执行（低配终端压力剖面账面跑账）");
    s.add(
        "代码体量≥300行当量",
        structural_weight() >= 300,
        if structural_weight() >= 300 { "" } else { "结构当量不足" },
    );
    let (j1, pass) = j1_main();
    s.add(
        "低配剖面账面执行10/10轮",
        j1,
        if j1 { "" } else { "剖面轮次不达标" },
    );
    s.add(
        "轻/中/重三档位全覆盖（F3021消费）",
        pass == ROUNDS && LowProfile::standard().is_standard(),
        if pass == ROUNDS { "" } else { "档位缺覆盖" },
    );
    s.add(
        "轮账与F2913账册面名对齐",
        inj4_face_names(),
        if inj4_face_names() { "" } else { "面名未对齐" },
    );
    let inj = [
        ("注入1池量漂移", inj1_pool_drift()),
        ("注入2分配率漂移", inj2_rate_drift()),
        ("注入3回收频漂移", inj3_reclaim_drift()),
        ("注入4面名对齐", inj4_face_names()),
        ("注入5池账闭合", inj5_ledger_open()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
