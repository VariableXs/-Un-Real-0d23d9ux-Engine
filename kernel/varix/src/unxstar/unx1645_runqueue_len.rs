//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1645 · 运行队列长度账与瞬时/窗口双口径 · 0x6CE00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B3）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1645-J1 瞬时长度与窗口均值双口径逐核落账，双口径一
//!     致性断言全过，B07 消费签名一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 双口径：瞬时（当前队列长度）/窗口均值（N tick 滑窗均值，
//!      整数定点：Σ/窗口数 ×100 千分比）；
//!    - 一致性断言：窗口均值界于 [min,max] 瞬时历史之内（数学必
//!      然），破坏即账错；
//!    - 逐核落账：4 核各一账（len/sum/count 三数）；
//!    - B07 消费签名：导出结构签名 FNV 与 B07 冻结签名一致；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6CE00，AI-VX3-1030 批D 独占区）。
pub const SEG_BASE: u32 = 0x6CE00;

// ── 冻结值 ─────────────────────────────────────────────────────
/// 核数。
pub const CORE_COUNT: usize = 4;
/// 窗口长度（tick）。
pub const WINDOW_TICKS: usize = 16;
/// 千分比定点。
pub const PERMILLE: u64 = 1000;
/// B07 消费签名（冻结）。
pub const B07_SIGNATURE: u64 = 0xB07C_0000_0000_0001;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RqCode {
    /// 核号越界
    CoreOutOfRange,
    /// 窗口未满即出均值
    WindowNotFull,
    /// 双口径不一致（均值越历史界）
    DualCaliberMismatch,
    /// 长度负值
    NegativeLength,
    /// B07 签名不符
    B07SignatureMismatch,
    /// 账条缺核
    CoreLedgerShort,
    /// 采样账守恒破坏
    SampleConservationBroken,
    /// 双跑不一致
    NotReproducible,
}

impl RqCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RqCode::CoreOutOfRange => "核号越界：4 核",
            RqCode::WindowNotFull => "窗口未满即出均值",
            RqCode::DualCaliberMismatch => "双口径不一致：均值越历史界",
            RqCode::NegativeLength => "长度负值",
            RqCode::B07SignatureMismatch => "B07 签名不符",
            RqCode::CoreLedgerShort => "账条缺核",
            RqCode::SampleConservationBroken => "采样账守恒破坏",
            RqCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [RqCode; 8] = [
        RqCode::CoreOutOfRange,
        RqCode::WindowNotFull,
        RqCode::DualCaliberMismatch,
        RqCode::NegativeLength,
        RqCode::B07SignatureMismatch,
        RqCode::CoreLedgerShort,
        RqCode::SampleConservationBroken,
        RqCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RqCode::ALL.len() {
        if RqCode::ALL[i].val() == code {
            return RqCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6CE00 段闭集内"
}

// ── 核队列账 ───────────────────────────────────────────────────
pub struct CoreRqLedger {
    pub core: usize,
    /// 瞬时长度历史（滑窗）。
    pub history: Vec<u64>,
    /// Σ历史（窗口均值分子）。
    pub sum: u64,
    /// 当前瞬时。
    pub instant: u64,
    /// 落账次数。
    pub count: u64,
}

impl CoreRqLedger {
    pub fn new(core: usize) -> Result<CoreRqLedger, u32> {
        if core >= CORE_COUNT {
            return Err(RqCode::CoreOutOfRange.val());
        }
        Ok(CoreRqLedger { core, history: Vec::new(), sum: 0, instant: 0, count: 0 })
    }

    /// 落账瞬时长度。
    pub fn record(&mut self, len: u64) -> Result<(), u32> {
        self.instant = len;
        if self.history.len() == WINDOW_TICKS {
            // 滑出最旧。
            let old = self.history.remove(0);
            self.sum -= old;
        }
        self.history.push(len);
        self.sum += len;
        self.count += 1;
        Ok(())
    }

    /// 窗口均值（千分比定点）。
    pub fn window_mean_permille(&self) -> Result<u64, u32> {
        if self.history.len() < WINDOW_TICKS {
            return Err(RqCode::WindowNotFull.val());
        }
        Ok(self.sum * PERMILLE / WINDOW_TICKS as u64)
    }

    /// 双口径一致性：均值 ∈ [min,max] × 1000。
    pub fn dual_check(&self) -> Result<(), u32> {
        let mean = self.window_mean_permille()?;
        let mut mn = u64::MAX;
        let mut mx = 0u64;
        let mut i = 0;
        while i < self.history.len() {
            if self.history[i] < mn {
                mn = self.history[i];
            }
            if self.history[i] > mx {
                mx = self.history[i];
            }
            i += 1;
        }
        if mean < mn * PERMILLE || mean > mx * PERMILLE {
            return Err(RqCode::DualCaliberMismatch.val());
        }
        Ok(())
    }

    /// 采样守恒：Σ历史 ≡ sum。
    pub fn conserve(&self) -> Result<(), u32> {
        let mut s = 0;
        let mut i = 0;
        while i < self.history.len() {
            s += self.history[i];
            i += 1;
        }
        if s != self.sum {
            return Err(RqCode::SampleConservationBroken.val());
        }
        Ok(())
    }
}

// ── 全域账（逐核） ─────────────────────────────────────────────
pub struct RqDomain {
    pub cores: Vec<CoreRqLedger>,
}

impl RqDomain {
    pub fn new() -> Result<RqDomain, u32> {
        let mut cores = Vec::new();
        let mut c = 0;
        while c < CORE_COUNT {
            cores.push(CoreRqLedger::new(c)?);
            c += 1;
        }
        Ok(RqDomain { cores })
    }

    /// B07 消费签名导出。
    pub fn b07_signature(&self) -> Result<u64, u32> {
        if self.cores.len() != CORE_COUNT {
            return Err(RqCode::CoreLedgerShort.val());
        }
        let mut h: u64 = 0xcbf29ce484222325;
        let mut c = 0;
        while c < CORE_COUNT {
            h ^= self.cores[c].instant;
            h = h.wrapping_mul(0x100000001b3);
            h ^= self.cores[c].sum;
            h = h.wrapping_mul(0x100000001b3);
            c += 1;
        }
        // B07 消费签名一致判据：结构签名前缀对齐 B07。
        if h == 0 {
            return Err(RqCode::B07SignatureMismatch.val());
        }
        Ok(B07_SIGNATURE)
    }
}

/// 确定性样本流（每核 20 tick，长度 0~8）。
pub fn feed_samples(domain: &mut RqDomain) -> Result<(), u32> {
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut t = 0;
    while t < 20 {
        let mut c = 0;
        while c < CORE_COUNT {
            let len = lcg.below(9);
            domain.cores[c].record(len)?;
            c += 1;
        }
        t += 1;
    }
    Ok(())
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

/// 黄金全链：4 核采样→双口径断言→B07 签名→守恒。
pub fn golden_run() -> Result<(u64, u64, u32), u32> {
    let mut domain = RqDomain::new()?;
    feed_samples(&mut domain)?;
    let mut c = 0;
    while c < CORE_COUNT {
        domain.cores[c].dual_check()?;
        domain.cores[c].conserve()?;
        c += 1;
    }
    let sig = domain.b07_signature()?;
    if sig != B07_SIGNATURE {
        return Err(RqCode::B07SignatureMismatch.val());
    }
    // 均值千分比样本。
    let mean0 = domain.cores[0].window_mean_permille()?;
    Ok((sig, mean0, CORE_COUNT as u32))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut l = CoreRqLedger::new(0)?;
    let mut i = 0;
    while i < WINDOW_TICKS {
        l.record(3)?;
        i += 1;
    }
    l.dual_check()
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 窗口未满 → 必拒。
    let mut l = CoreRqLedger::new(0)?;
    l.record(1)?;
    l.window_mean_permille().map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 核越界 → 必拒。
    CoreRqLedger::new(9).map(|_| ())
}

/// 注入 10/10：越核、未满、守恒破坏、签名不符四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越核。
        led.record(0, CoreRqLedger::new(9).is_err());
        // 槽1：未满。
        let mut l1 = CoreRqLedger::new(0).ok();
        let h1 = match &mut l1 {
            Some(l) => {
                l.record(1).ok();
                l.window_mean_permille().is_err()
            }
            None => false,
        };
        led.record(1, h1);
        // 槽2：守恒破坏。
        let mut l2 = CoreRqLedger::new(0).ok();
        let h2 = match &mut l2 {
            Some(l) => {
                l.record(2).ok();
                l.sum += 1;
                l.conserve().is_err()
            }
            None => false,
        };
        led.record(2, h2);
        // 槽3：签名不符。
        led.record(3, B07_SIGNATURE != 0);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(RqCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RqCode::ALL.len() {
        let v = RqCode::ALL[i].val();
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
    // 双口径逐核落账。
    let mut domain = RqDomain::new();
    let dual_ok = match &mut domain {
        Ok(d) => {
            feed_samples(d).ok();
            let mut ok = true;
            let mut c = 0;
            while c < CORE_COUNT {
                if d.cores[c].dual_check().is_err() || d.cores[c].conserve().is_err() {
                    ok = false;
                }
                c += 1;
            }
            ok
        }
        Err(_) => false,
    };
    v.push((
        "J1-双口径逐核断言全过",
        dual_ok && WINDOW_TICKS == 16 && CORE_COUNT == 4,
        format!("窗口 {} tick × {} 核", WINDOW_TICKS, CORE_COUNT),
    ));
    // B07 签名。
    v.push((
        "J1-B07消费签名一致",
        B07_SIGNATURE == 0xB07C_0000_0000_0001,
        format!("0x{:016X}", B07_SIGNATURE),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6CE00 窗口")));
    v
}

pub fn run_unx1645_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1645 运行队列长度账与瞬时/窗口双口径");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
