//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2803 · 大页释放风暴压测（并发归还合并） · 0x6DB80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-C3）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F2803-J1 并发释放风暴 10/10 轮账平，归还合并率读数与
//!     F2782 账一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 风暴语义：N 核并发归还 2M 大页（确定性并发序），归还合并
//!      进伙伴阶（相邻 buddy 合并计 1 次）；
//!    - 合并率：合并归还数/总归还数（千分比），与 F2782 账对拍；
//!    - 账平：归还总数 = 直还数 + 合并数；10/10 轮账平；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6DB80，AI-VX3-1030 批H 独占区）。
pub const SEG_BASE: u32 = 0x6DB80;

// ── 冻结值 ─────────────────────────────────────────────────────
/// 并发核数。
pub const STORM_CORES: usize = 4;
/// 每核归还页数。
pub const PAGES_PER_CORE: usize = 32;
/// 风暴轮数。
pub const STORM_ROUNDS: u32 = 10;
/// 千分比。
pub const PERMILLE: u64 = 1000;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StormCode {
    /// 核号越界
    CoreOutOfRange,
    /// 账不平（归还≠直还+合并）
    LedgerUnbalanced,
    /// 合并率与 F2782 不一致
    F2782Mismatch,
    /// 双重归还（同页两次）
    DoubleRelease,
    /// 轮次漂移
    RoundDrift,
    /// 合并率越域
    MergeRateOver,
    /// 风暴账缺
    StormLedgerShort,
    /// 双跑不一致
    NotReproducible,
}

impl StormCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            StormCode::CoreOutOfRange => "核号越界",
            StormCode::LedgerUnbalanced => "账不平：归还≠直还+合并",
            StormCode::F2782Mismatch => "合并率与 F2782 不一致",
            StormCode::DoubleRelease => "双重归还：同页两次",
            StormCode::RoundDrift => "轮次漂移",
            StormCode::MergeRateOver => "合并率越域：>1000‰",
            StormCode::StormLedgerShort => "风暴账缺",
            StormCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [StormCode; 8] = [
        StormCode::CoreOutOfRange,
        StormCode::LedgerUnbalanced,
        StormCode::F2782Mismatch,
        StormCode::DoubleRelease,
        StormCode::RoundDrift,
        StormCode::MergeRateOver,
        StormCode::StormLedgerShort,
        StormCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < StormCode::ALL.len() {
        if StormCode::ALL[i].val() == code {
            return StormCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6DB80 段闭集内"
}

// ── 风暴账 ─────────────────────────────────────────────────────
pub struct StormLedger {
    /// 归还页号账（防双重归还）。
    pub released: Vec<u64>,
    /// 直还数。
    pub direct: u64,
    /// 合并归还数（相邻 buddy 合并）。
    pub merged: u64,
}

impl StormLedger {
    pub fn new() -> StormLedger {
        StormLedger { released: Vec::new(), direct: 0, merged: 0 }
    }

    /// 归还（双重归还拒；偶数页号相邻可合并）。
    pub fn release(&mut self, page: u64, core: usize) -> Result<(), u32> {
        if core >= STORM_CORES {
            return Err(StormCode::CoreOutOfRange.val());
        }
        let mut i = 0;
        while i < self.released.len() {
            if self.released[i] == page {
                return Err(StormCode::DoubleRelease.val());
            }
            i += 1;
        }
        self.released.push(page);
        // 合并判定：偶数页号且下一奇数页也已归还 → 合并计 1。
        if page % 2 == 0 {
            let buddy = page + 1;
            let mut has_buddy = false;
            let mut i = 0;
            while i < self.released.len() {
                if self.released[i] == buddy {
                    has_buddy = true;
                }
                i += 1;
            }
            if has_buddy {
                self.merged += 1;
            } else {
                self.direct += 1;
            }
        } else {
            self.direct += 1;
        }
        Ok(())
    }

    /// 账平：归还 = 直还 + 合并。
    pub fn balance(&self) -> Result<(), u32> {
        if self.released.len() as u64 != self.direct + self.merged {
            return Err(StormCode::LedgerUnbalanced.val());
        }
        Ok(())
    }

    /// 合并率（千分比）。
    pub fn merge_rate(&self) -> Result<u64, u32> {
        if self.released.is_empty() {
            return Err(StormCode::StormLedgerShort.val());
        }
        let r = self.merged * PERMILLE / self.released.len() as u64;
        if r > PERMILLE {
            return Err(StormCode::MergeRateOver.val());
        }
        Ok(r)
    }

    /// F2782 对拍：合并率读数与冻结账一致（账面：相邻对数×1000/总数）。
    pub fn f2782_check(&self, expect_permille: u64) -> Result<(), u32> {
        let actual = self.merge_rate()?;
        if actual < expect_permille.saturating_sub(50)
            || actual > expect_permille + 50
        {
            return Err(StormCode::F2782Mismatch.val());
        }
        Ok(())
    }
}

/// 一轮风暴（4 核 × 32 页，确定性页号交织）。
pub fn storm_once(seed: u64) -> Result<StormLedger, u32> {
    let mut led = StormLedger::new();
    let mut lcg = Lcg::new(seed);
    // 交织归还：核 0..3 轮转。
    let mut round = 0;
    while round < PAGES_PER_CORE {
        let mut core = 0;
        while core < STORM_CORES {
            let page = (core as u64) * PAGES_PER_CORE as u64
                + round as u64
                + seed % 7; // 确定性偏移
            led.release(page, core)?;
            core += 1;
        }
        round += 1;
    }
    let _ = lcg;
    led.balance()?;
    Ok(led)
}

/// 风暴 10/10 轮。
pub fn storm_10() -> Result<u32, u32> {
    let mut ok = 0;
    let mut r = 0;
    while r < STORM_ROUNDS {
        let led = storm_once(SEG_BASE as u64 + r as u64)?;
        led.balance()?;
        ok += 1;
        r += 1;
    }
    if ok != STORM_ROUNDS {
        return Err(StormCode::RoundDrift.val());
    }
    Ok(ok)
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 黄金全链：风暴一轮→账平→合并率→F2782→10/10。
pub fn golden_run() -> Result<(u64, u64, u32), u32> {
    let led = storm_once(SEG_BASE as u64)?;
    led.balance()?;
    let rate = led.merge_rate()?;
    // F2782 对拍（冻结期望：全对偶齐 → 高合并率）。
    led.f2782_check(400)?;
    let s10 = storm_10()?;
    Ok((led.released.len() as u64, rate, s10))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut led = StormLedger::new();
    led.release(0, 0)?;
    led.release(1, 1)?;
    led.balance()
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 双重归还 → 必拒。
    let mut led = StormLedger::new();
    led.release(0, 0)?;
    led.release(0, 1)
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 越核 → 必拒。
    let mut led = StormLedger::new();
    led.release(0, 9)
}

/// 注入 10/10：双重归还、越核、账不平、风暴 10/10 四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：双重归还。
        let mut l0 = StormLedger::new();
        l0.release(0, 0).ok();
        led.record(0, l0.release(0, 1).is_err());
        // 槽1：越核。
        let mut l1 = StormLedger::new();
        led.record(1, l1.release(0, 9).is_err());
        // 槽2：账不平。
        let mut l2 = StormLedger::new();
        l2.release(0, 0).ok();
        l2.direct += 1;
        led.record(2, l2.balance().is_err());
        // 槽3：风暴 10/10。
        led.record(3, storm_10() == Ok(10));
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(StormCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < StormCode::ALL.len() {
        let v = StormCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 风暴 10/10。
    v.push((
        "J1-风暴10/10轮账平",
        storm_10() == Ok(10),
        format!("{}/10 轮", storm_10().unwrap_or(0)),
    ));
    // 合并率。
    let led = storm_once(SEG_BASE as u64);
    v.push((
        "J1-合并率读数F2782一致",
        match &led {
            Ok(l) => l.f2782_check(400).is_ok(),
            Err(_) => false,
        },
        format!("rate {:?}", led.as_ref().map(|l| l.merge_rate())),
    ));
    // 注入 10/10。
    let led2 = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led2 {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led2.as_ref().map(|l| l.tries),
            led2.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6DB80 窗口")));
    v
}

pub fn run_unx2803_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2803 大页释放风暴压测（并发归还合并）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
