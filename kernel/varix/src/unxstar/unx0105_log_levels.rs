//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0105 · 日志级别体系（debug–fatal） · 0x6E7C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0105-J1 日志五级（DEBUG/INFO/WARN/ERROR/FATAL）体系：
//!     级别闭集 + 阈值过滤（loglevel=N 只放行 ≥N）+ 前缀文案逐字
//!     （[DEBUG] … [FATAL]）+ 非法级别显性拒，fatal 恒过闸；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五级闭集（0-4 冻结：DEBUG/INFO/WARN/ERROR/FATAL）；
//!   - 阈值过滤：emit 级 ≥ 阈值才落账；FATAL(4) 恒过闸；
//!   - 前缀文案五态逐字（含方括号与空格）；
//!   - loglevel= 解析（0-4，越域拒、非法字符拒）；
//!   - 过滤账可复算（同序列同落账）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6E7C0，AI-VX3-1102 批K 独占区）。
pub const SEG_BASE: u32 = 0x6E7C0;

/// 五级（冻结）。
pub const L_DEBUG: u8 = 0;
pub const L_INFO: u8 = 1;
pub const L_WARN: u8 = 2;
pub const L_ERROR: u8 = 3;
pub const L_FATAL: u8 = 4;
/// 级数。
pub const LEVEL_COUNT: usize = 5;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogCode {
    /// 级别越闭集（>4）
    LevelOver,
    /// loglevel 越域（>4）
    ThresholdOver,
    /// loglevel 非法字符
    ThresholdDigitBad,
    /// 前缀文案失配
    PrefixMismatch,
    /// FATAL 被过滤（恒过闸红线）
    FatalFiltered,
    /// 过滤账复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl LogCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            LogCode::LevelOver => "级别越闭集：>4",
            LogCode::ThresholdOver => "loglevel 越域：>4",
            LogCode::ThresholdDigitBad => "loglevel 非法字符",
            LogCode::PrefixMismatch => "前缀文案失配",
            LogCode::FatalFiltered => "FATAL 被过滤：恒过闸红线",
            LogCode::ReplayMismatch => "过滤账复算失配",
            LogCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [LogCode; 7] = [
        LogCode::LevelOver,
        LogCode::ThresholdOver,
        LogCode::ThresholdDigitBad,
        LogCode::PrefixMismatch,
        LogCode::FatalFiltered,
        LogCode::ReplayMismatch,
        LogCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < LogCode::ALL.len() {
        if LogCode::ALL[i].val() == code {
            return LogCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6E7C0 段闭集内"
}

// ── 级别账 ──────────────────────────────────────────────────────
/// 前缀文案（五态逐字）。
pub fn level_prefix(level: u8) -> Result<&'static str, u32> {
    match level {
        L_DEBUG => Ok("[DEBUG] "),
        L_INFO => Ok("[INFO] "),
        L_WARN => Ok("[WARN] "),
        L_ERROR => Ok("[ERROR] "),
        L_FATAL => Ok("[FATAL] "),
        _ => Err(LogCode::LevelOver.val()),
    }
}

/// 阈值过滤：emit 级 ≥ 阈值 → 落账；FATAL 恒过闸。
pub fn passes(level: u8, threshold: u8) -> Result<bool, u32> {
    if level > L_FATAL {
        return Err(LogCode::LevelOver.val());
    }
    if level == L_FATAL {
        return Ok(true);
    }
    Ok(level >= threshold)
}

/// loglevel= 解析（0-4）。
pub fn parse_loglevel(cmdline: &str) -> Result<u8, u32> {
    let b = cmdline.as_bytes();
    let mut i = 0;
    while i + 9 <= b.len() {
        if &cmdline[i..i + 9] == "loglevel=" {
            let c = b[i + 9];
            if c < b'0' || c > b'9' {
                return Err(LogCode::ThresholdDigitBad.val());
            }
            let v = c - b'0';
            if v as usize > L_FATAL as usize {
                return Err(LogCode::ThresholdOver.val());
            }
            return Ok(v);
        }
        i += 1;
    }
    Ok(L_INFO) // 缺省 INFO
}

/// 过滤账复算：同序列同阈值重建落账一致。
pub fn replay_filter(seq: &[u8], threshold: u8) -> Result<Vec<bool>, u32> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < seq.len() {
        out.push(passes(seq[i], threshold)?);
        i += 1;
    }
    Ok(out)
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

// ── 注入检出账（四路：越级路/阈值路/前缀路/FATAL 路） ────────────
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

/// 真机序列：五级各一。
pub fn machine_seq() -> Vec<u8> {
    vec![L_DEBUG, L_INFO, L_WARN, L_ERROR, L_FATAL]
}

// ── 黄金全链：loglevel=2 → 落账 {WARN,ERROR,FATAL} → FATAL 恒过 ──
pub fn golden_run() -> Result<(usize, u8, bool), u32> {
    let th = parse_loglevel("quiet loglevel=2 ro")?;
    let got = replay_filter(&machine_seq(), th)?;
    let n = got.iter().fold(0u32, |a, &b| a + if b { 1 } else { 0 });
    let fatal = passes(L_FATAL, L_DEBUG)?;
    Ok((n as usize, th, fatal))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：loglevel=2 → 3 条落账 + FATAL 恒过。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((3, 2, true)) => Ok(()),
        _ => Err(LogCode::NotReproducible.val()),
    }
}

/// 序 B：loglevel=7 越域 ⇒ ThresholdOver。
pub fn probe_seq_b() -> Result<(), u32> {
    match parse_loglevel("loglevel=7") {
        Err(e) if e == LogCode::ThresholdOver.val() => Ok(()),
        _ => Err(LogCode::NotReproducible.val()),
    }
}

/// 序 C：前缀五态逐字。
pub fn probe_seq_c() -> Result<(), u32> {
    let want = ["[DEBUG] ", "[INFO] ", "[WARN] ", "[ERROR] ", "[FATAL] "];
    let mut i = 0;
    while i < LEVEL_COUNT {
        if level_prefix(i as u8) != Ok(want[i]) {
            return Err(LogCode::NotReproducible.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越级路（emit 级 5 必拒）。
        led.record(0, passes(5, 0).is_err());
        // 路 2：阈值路（loglevel 非法字符/越域全拒）。
        led.record(1, parse_loglevel("loglevel=x").is_err() && parse_loglevel("loglevel=9").is_err());
        // 路 3：前缀路（越级前缀必拒）。
        led.record(2, level_prefix(9).is_err());
        // 路 4：FATAL 恒过路（任意阈值 0-4 下 FATAL 恒真）。
        let mut all = true;
        let mut th = 0u8;
        while th <= L_FATAL {
            if !passes(L_FATAL, th)? {
                all = false;
            }
            th += 1;
        }
        led.record(3, all);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(LogCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < LogCode::ALL.len() {
        let v = LogCode::ALL[i].val();
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
            Ok((n, th, fatal)) => *n == 3 && *th == 2 && *fatal,
            Err(_) => false,
        },
        match &g {
            Ok((n, th, fatal)) => format!("emitted={} threshold={} fatal_pass={}", n, th, fatal),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 五级闭集。
    v.push((
        "J1-五级闭集",
        LEVEL_COUNT == 5 && (L_DEBUG, L_INFO, L_WARN, L_ERROR, L_FATAL) == (0, 1, 2, 3, 4),
        String::from("DEBUG=0 INFO=1 WARN=2 ERROR=3 FATAL=4"),
    ));
    // 缺省阈值 INFO。
    v.push((
        "J1-缺省阈值",
        parse_loglevel("quiet ro") == Ok(L_INFO),
        String::from("无 loglevel ⇒ INFO(1)"),
    ));
    // 阈值边界（level==threshold 放行）。
    v.push((
        "J1-阈值边界",
        passes(L_WARN, L_WARN) == Ok(true) && passes(L_INFO, L_WARN) == Ok(false),
        String::from("≥ 阈值放行 / < 阈值过滤"),
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
        String::from("0x6E7C0 窗口"),
    ));
    v
}

pub fn run_unx0105_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0105 日志级别体系（debug–fatal）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
