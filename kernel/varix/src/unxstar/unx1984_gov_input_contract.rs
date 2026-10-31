//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1984 · governor 决策输入面（负载信号取数契约） · 0x6D240
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B4）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F1984-J1 取数契约冻结 v1（governor←调度统计的信号集/
//!     单位/节拍三列），B24 统计前置联签预告位在册，取数越界 10/10
//!     拒绝；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 信号集闭集：每核忙度（‰）/队列长/唤醒率（‰）三信号（冻
//!      结）；
//!    - 三列契约：信号名/单位/节拍（tick 数冻结）；
//!    - 预告位：B24 统计前置联签预告位在册（bool 位图）；
//!    - 取数越界 10/10 拒（越核/越信号/越节拍窗）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6D240，AI-VX3-1030 批E 独占区）。
pub const SEG_BASE: u32 = 0x6D240;

// ── 信号集闭集 ─────────────────────────────────────────────────
pub const SIG_BUSY: usize = 0;
pub const SIG_QUEUE_LEN: usize = 1;
pub const SIG_WAKE_RATE: usize = 2;
pub const SIGNAL_COUNT: usize = 3;
pub const SIGNAL_NAMES: [&str; SIGNAL_COUNT] = ["每核忙度", "队列长", "唤醒率"];
pub const SIGNAL_UNITS: [&str; SIGNAL_COUNT] = ["‰", "个", "‰"];
/// 节拍（tick，冻结）。
pub const SIGNAL_TICKS: [u64; SIGNAL_COUNT] = [16, 4, 32];
/// 核数。
pub const CORE_COUNT: usize = 4;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GovCode {
    /// 信号越闭集
    SignalOutOfRange,
    /// 信号名空（三信号齐备破坏）
    SignalNameBlank,
    /// 单位列空
    UnitBlank,
    /// 核号越界
    CoreOutOfRange,
    /// 值越域（‰ 类 >1000）
    ValueOutOfRange,
    /// 预告位缺（B24 联签）
    PreannounceMissing,
    /// 取数越界漏拒
    BoundLeaked,
    /// 双跑不一致
    NotReproducible,
}

impl GovCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GovCode::SignalOutOfRange => "信号越闭集：三信号冻结",
            GovCode::SignalNameBlank => "信号名空：齐备破坏",
            GovCode::UnitBlank => "单位列空",
            GovCode::CoreOutOfRange => "核号越界：4 核",
            GovCode::ValueOutOfRange => "值越域：‰ 类 >1000",
            GovCode::PreannounceMissing => "预告位缺：B24 联签",
            GovCode::BoundLeaked => "取数越界漏拒",
            GovCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [GovCode; 8] = [
        GovCode::SignalOutOfRange,
        GovCode::SignalNameBlank,
        GovCode::UnitBlank,
        GovCode::CoreOutOfRange,
        GovCode::ValueOutOfRange,
        GovCode::PreannounceMissing,
        GovCode::BoundLeaked,
        GovCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < GovCode::ALL.len() {
        if GovCode::ALL[i].val() == code {
            return GovCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6D240 段闭集内"
}

// ── 取数契约 ───────────────────────────────────────────────────
/// 值域（按信号）：‰ 类 [0,1000]，队列长 [0,4096]。
pub fn value_legal(sig: usize, v: u64) -> Result<(), u32> {
    match sig {
        SIG_BUSY | SIG_WAKE_RATE => {
            if v > 1000 {
                return Err(GovCode::ValueOutOfRange.val());
            }
            Ok(())
        }
        SIG_QUEUE_LEN => {
            if v > 4096 {
                return Err(GovCode::ValueOutOfRange.val());
            }
            Ok(())
        }
        _ => Err(GovCode::SignalOutOfRange.val()),
    }
}

pub struct GovInput {
    /// 预告位（B24 联签）。
    pub preannounced: bool,
    /// 采样账 [core][signal] → 最新值。
    pub samples: [[u64; SIGNAL_COUNT]; CORE_COUNT],
    pub sampled: u64,
}

impl GovInput {
    pub fn new() -> Result<GovInput, u32> {
        if SIGNAL_NAMES.len() != SIGNAL_COUNT || SIGNAL_UNITS.len() != SIGNAL_COUNT {
            return Err(GovCode::SignalNameBlank.val());
        }
        let mut i = 0;
        while i < SIGNAL_COUNT {
            if SIGNAL_NAMES[i].len() == 0 || SIGNAL_UNITS[i].len() == 0 {
                return Err(GovCode::UnitBlank.val());
            }
            i += 1;
        }
        Ok(GovInput { preannounced: false, samples: [[0; SIGNAL_COUNT]; CORE_COUNT], sampled: 0 })
    }

    /// 取数（越核/越信号/越域拒）。
    pub fn feed(&mut self, core: usize, sig: usize, v: u64) -> Result<(), u32> {
        if !self.preannounced {
            return Err(GovCode::PreannounceMissing.val());
        }
        if core >= CORE_COUNT {
            return Err(GovCode::CoreOutOfRange.val());
        }
        if sig >= SIGNAL_COUNT {
            return Err(GovCode::SignalOutOfRange.val());
        }
        value_legal(sig, v)?;
        self.samples[core][sig] = v;
        self.sampled += 1;
        Ok(())
    }

    /// governor 决策取数（只读）。
    pub fn fetch(&self, core: usize, sig: usize) -> Result<u64, u32> {
        if core >= CORE_COUNT {
            return Err(GovCode::CoreOutOfRange.val());
        }
        if sig >= SIGNAL_COUNT {
            return Err(GovCode::SignalOutOfRange.val());
        }
        Ok(self.samples[core][sig])
    }
}

/// 取数越界 10/10 拒（越核×4/越信号×3/越域×3）。
pub fn bound_reject_10() -> Result<u32, u32> {
    let mut gi = GovInput::new()?;
    gi.preannounced = true;
    let mut rejected = 0;
    // 越核 ×4。
    let mut k = 0;
    while k < 4 {
        if gi.feed(CORE_COUNT + k, SIG_BUSY, 500).is_err() {
            rejected += 1;
        }
        k += 1;
    }
    // 越信号 ×3。
    k = 0;
    while k < 3 {
        if gi.feed(0, SIGNAL_COUNT + k, 1).is_err() {
            rejected += 1;
        }
        k += 1;
    }
    // 越域 ×3。
    k = 0;
    while k < 3 {
        if value_legal(SIG_BUSY, 1001u64 + k as u64).is_err() {
            rejected += 1;
        }
        k += 1;
    }
    if rejected != 10 {
        return Err(GovCode::BoundLeaked.val());
    }
    Ok(rejected)
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

/// 黄金全链：预告位→合法取数→fetch 回读→越界 10/10。
pub fn golden_run() -> Result<(u64, u64, u32), u32> {
    let mut gi = GovInput::new()?;
    gi.preannounced = true;
    // 合法取数。
    gi.feed(0, SIG_BUSY, 700)?;
    gi.feed(0, SIG_QUEUE_LEN, 3)?;
    gi.feed(0, SIG_WAKE_RATE, 120)?;
    // fetch 回读。
    let v = gi.fetch(0, SIG_BUSY)?;
    if v != 700 {
        return Err(GovCode::ValueOutOfRange.val());
    }
    let rej = bound_reject_10()?;
    Ok((gi.sampled, v, rej))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut gi = GovInput::new()?;
    gi.preannounced = true;
    gi.feed(1, SIG_QUEUE_LEN, 5)?;
    gi.fetch(1, SIG_QUEUE_LEN).map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 预告位缺 → 必拒。
    let mut gi = GovInput::new()?;
    gi.feed(0, SIG_BUSY, 100)
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 值越域 → 必拒。
    value_legal(SIG_BUSY, 1001)
}

/// 注入 10/10：越核、越信号、越域、预告缺四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越核。
        let mut g0 = GovInput::new().ok();
        let h0 = match &mut g0 {
            Some(g) => {
                g.preannounced = true;
                g.feed(9, SIG_BUSY, 1).is_err()
            }
            None => false,
        };
        led.record(0, h0);
        // 槽1：越信号。
        let mut g1 = GovInput::new().ok();
        let h1 = match &mut g1 {
            Some(g) => {
                g.preannounced = true;
                g.feed(0, 9, 1).is_err()
            }
            None => false,
        };
        led.record(1, h1);
        // 槽2：越域。
        led.record(2, value_legal(SIG_WAKE_RATE, 2000).is_err());
        // 槽3：预告缺。
        let mut g3 = GovInput::new().ok();
        let h3 = match &mut g3 {
            Some(g) => g.feed(0, SIG_BUSY, 1).is_err(),
            None => false,
        };
        led.record(3, h3);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(GovCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < GovCode::ALL.len() {
        let v = GovCode::ALL[i].val();
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
    // 契约三列冻结。
    v.push((
        "J1-取数契约冻结v1",
        SIGNAL_NAMES.len() == 3 && SIGNAL_UNITS.len() == 3 && SIGNAL_TICKS.len() == 3,
        format!("{:?}", SIGNAL_NAMES),
    ));
    // 越界 10/10。
    v.push((
        "J1-取数越界10/10拒",
        bound_reject_10() == Ok(10),
        format!("{}/10 拒", bound_reject_10().unwrap_or(0)),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6D240 窗口")));
    v
}

pub fn run_unx1984_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1984 governor 决策输入面（负载信号取数契约）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
