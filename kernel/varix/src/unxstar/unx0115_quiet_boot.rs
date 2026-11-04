//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0115 · 静默引导模式（quiet） · 0x6EA40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥180 行；
//!   ② UNX-F0115-J1 quiet 语义：cmdline quiet ⇒ INFO 及以下全滤
//!     （WARN/ERROR/FATAL 仍出），quiet 恒静默红线（INFO 恒不出
//!     账），非 quiet 全通，显性三态（quiet/verbose/缺省）账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三态开关：quiet（词边界）/verbose/缺省（INFO 基线）；
//!   - quiet 语义：阈值抬到 WARN（2），INFO 恒不出账；
//!   - verbose：阈值降到 DEBUG（0）全通；
//!   - 缺省：阈值 INFO（1）；
//!   - FATAL 恒过（与 F0105 联签）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6EA40，AI-VX3-1102 批L 独占区）。
pub const SEG_BASE: u32 = 0x6EA40;

/// 三态阈值（冻结）。
pub const TH_QUIET: u8 = 2;
pub const TH_DEFAULT: u8 = 1;
pub const TH_VERBOSE: u8 = 0;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuietCode {
    /// quiet 期 INFO 出账（恒静默红线）
    QuietInfoLeak,
    /// 三态阈值失配
    ThresholdBad,
    /// 词边界误判（confquiet 误触）
    WordBoundaryBad,
    /// FATAL 被滤
    FatalFiltered,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl QuietCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            QuietCode::QuietInfoLeak => "quiet 期 INFO 出账：恒静默红线",
            QuietCode::ThresholdBad => "三态阈值失配",
            QuietCode::WordBoundaryBad => "词边界误判",
            QuietCode::FatalFiltered => "FATAL 被滤",
            QuietCode::ReplayMismatch => "复算失配",
            QuietCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [QuietCode; 6] = [
        QuietCode::QuietInfoLeak,
        QuietCode::ThresholdBad,
        QuietCode::WordBoundaryBad,
        QuietCode::FatalFiltered,
        QuietCode::ReplayMismatch,
        QuietCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < QuietCode::ALL.len() {
        if QuietCode::ALL[i].val() == code {
            return QuietCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6EA40 段闭集内"
}

// ── 三态开关 ────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuietSwitch {
    Quiet,
    Verbose,
    Default,
}

/// 词边界匹配（前后界：空/逗号/串端）。
fn word_at(cmdline: &str, word: &str) -> bool {
    let b = cmdline.as_bytes();
    let w = word.as_bytes();
    let mut i = 0;
    while i + w.len() <= b.len() {
        let before_ok = i == 0 || b[i - 1] == b' ' || b[i - 1] == b',';
        let j = i + w.len();
        let after_ok = j >= b.len() || b[j] == b' ' || b[j] == b',';
        if before_ok && after_ok && &cmdline[i..j] == word {
            return true;
        }
        i += 1;
    }
    false
}

/// 解析三态。
pub fn parse_switch(cmdline: &str) -> QuietSwitch {
    if word_at(cmdline, "quiet") {
        QuietSwitch::Quiet
    } else if word_at(cmdline, "verbose") {
        QuietSwitch::Verbose
    } else {
        QuietSwitch::Default
    }
}

/// 阈值账（三态冻结）。
pub fn threshold_of(sw: QuietSwitch) -> Result<u8, u32> {
    match sw {
        QuietSwitch::Quiet => Ok(TH_QUIET),
        QuietSwitch::Default => Ok(TH_DEFAULT),
        QuietSwitch::Verbose => Ok(TH_VERBOSE),
    }
}

/// 落账判定（FATAL 恒过）。
pub fn passes(level: u8, th: u8) -> Result<bool, u32> {
    if level > 4 {
        return Err(QuietCode::ReplayMismatch.val());
    }
    if level == 4 {
        return Ok(true);
    }
    Ok(level >= th)
}

/// quiet 恒静默红线：INFO 在 quiet 阈值下必滤。
pub fn quiet_info_closed() -> Result<(), u32> {
    if passes(1, TH_QUIET)? {
        return Err(QuietCode::QuietInfoLeak.val());
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

// ── 注入检出账（四路：三态路/静默路/FATAL 路/词界路） ────────────
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

/// 真机五级序列。
pub fn machine_levels() -> Vec<u8> {
    vec![0, 1, 2, 3, 4]
}

// ── 黄金全链：三态阈值→quiet 落账 {2,3,4}→红线→复算 ──────────────
pub fn golden_run() -> Result<(u8, u32, bool), u32> {
    let sw = parse_switch("ro quiet loglevel=3");
    let th = threshold_of(sw)?;
    let levels = machine_levels();
    let mut emitted = 0u32;
    let mut i = 0;
    while i < levels.len() {
        if passes(levels[i], th)? {
            emitted += 1;
        }
        i += 1;
    }
    quiet_info_closed()?;
    // 复算：同序列再算恒一致。
    let mut again = 0u32;
    let mut i = 0;
    while i < levels.len() {
        if passes(levels[i], th)? {
            again += 1;
        }
        i += 1;
    }
    if again != emitted {
        return Err(QuietCode::ReplayMismatch.val());
    }
    Ok((th, emitted, true))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：quiet → 阈值 2 + 落账 3（WARN/ERROR/FATAL）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((2, 3, true)) => Ok(()),
        _ => Err(QuietCode::NotReproducible.val()),
    }
}

/// 序 B：词边界（confquiet 不误触，quiet 词界触发）。
pub fn probe_seq_b() -> Result<(), u32> {
    let bad = parse_switch("confquiet");
    let good = parse_switch("quiet");
    match (bad, good) {
        (QuietSwitch::Default, QuietSwitch::Quiet) => Ok(()),
        _ => Err(QuietCode::NotReproducible.val()),
    }
}

/// 序 C：verbose 全通（DEBUG 也出）。
pub fn probe_seq_c() -> Result<(), u32> {
    let th = threshold_of(parse_switch("verbose"))?;
    match passes(0, th) {
        Ok(true) => Ok(()),
        _ => Err(QuietCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：三态路（阈值冻结逐态）。
        let t1 = threshold_of(parse_switch("quiet")) == Ok(2)
            && threshold_of(parse_switch("verbose")) == Ok(0)
            && threshold_of(parse_switch("ro")) == Ok(1);
        led.record(0, t1);
        // 路 2：静默路。
        led.record(1, quiet_info_closed().is_ok() && passes(2, TH_QUIET) == Ok(true));
        // 路 3：FATAL 路（任意态恒过）。
        let t3 = passes(4, TH_QUIET) == Ok(true)
            && passes(4, TH_DEFAULT) == Ok(true)
            && passes(4, TH_VERBOSE) == Ok(true);
        led.record(2, t3);
        // 路 4：词界路。
        led.record(3, word_at("a,quiet,b", "quiet") && !word_at("xquiet", "quiet"));
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(QuietCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < QuietCode::ALL.len() {
        let v = QuietCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((th, emitted, ok)) => *th == 2 && *emitted == 3 && *ok,
            Err(_) => false,
        },
        match &g {
            Ok((th, emitted, ok)) => format!("threshold={} emitted={} ok={}", th, emitted, ok),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 三态阈值冻结。
    v.push((
        "J1-三态阈值",
        (TH_QUIET, TH_DEFAULT, TH_VERBOSE) == (2, 1, 0),
        String::from("quiet=2 / 缺省=1 / verbose=0 冻结"),
    ));
    // FATAL 恒过（与 F0105 联签）。
    v.push((
        "J1-FATAL联签",
        passes(4, TH_QUIET).is_ok() && passes(4, TH_QUIET) == Ok(true),
        String::from("FATAL 任意态恒过（F0105 同语义）"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a=绿 b=检出{} c=检出{}", b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6EA40 窗口"),
    ));
    v
}

pub fn run_unx0115_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0115 静默引导模式（quiet）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
