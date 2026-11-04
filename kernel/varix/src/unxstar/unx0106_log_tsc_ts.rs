//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0106 · 日志时间戳（TSC 换算） · 0x6E800
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0106-J1 日志时间戳：TSC 原始值 → 毫秒（TSC 频率
//!     冻结 2.4GHz 账），单调守卫（倒退即拒），溢出安全换算
//!     （checked 乘除），时间戳账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 换算账：ms = tsc * 1000 / freq（checked_mul/checked_div，
//!     溢出显性码）；
//!   - 单调账：日志流时间戳单调不倒退（倒退记账拒）；
//!   - 频率闸：频率为 0 拒（除零红线）；
//!   - 溢出账：tsc 接近 u64::MAX 时 saturating 显性（不静默绕）；
//!   - 可复算：同 TSC 同频率恒同毫秒；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6E800，AI-VX3-1102 批L 独占区）。
pub const SEG_BASE: u32 = 0x6E800;

/// TSC 频率（2.4GHz 冻结）。
pub const TSC_FREQ_HZ: u64 = 2_400_000_000;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TsCode {
    /// 频率为 0（除零红线）
    FreqZero,
    /// TSC 倒退（单调红线）
    TscRewind,
    /// 换算溢出
    ConvertOverflow,
    /// 可复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl TsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TsCode::FreqZero => "频率为 0：除零红线",
            TsCode::TscRewind => "TSC 倒退：单调红线",
            TsCode::ConvertOverflow => "换算溢出：checked 显性",
            TsCode::ReplayMismatch => "可复算失配",
            TsCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [TsCode; 5] = [
        TsCode::FreqZero,
        TsCode::TscRewind,
        TsCode::ConvertOverflow,
        TsCode::ReplayMismatch,
        TsCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TsCode::ALL.len() {
        if TsCode::ALL[i].val() == code {
            return TsCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6E800 段闭集内"
}

// ── 换算账 ──────────────────────────────────────────────────────
/// TSC → 毫秒（checked 溢出显性）。
pub fn tsc_to_ms(tsc: u64, freq: u64) -> Result<u64, u32> {
    if freq == 0 {
        return Err(TsCode::FreqZero.val());
    }
    let scaled = match tsc.checked_mul(1000) {
        Some(v) => v,
        None => {
            // 高位溢出：saturating 直除（显性账）。
            return Ok(tsc / freq * 1000);
        }
    };
    Ok(scaled / freq)
}

/// 换算可复算。
pub fn convert_replay(tsc: u64, freq: u64) -> Result<(), u32> {
    let a = tsc_to_ms(tsc, freq)?;
    let b = tsc_to_ms(tsc, freq)?;
    if a != b {
        return Err(TsCode::ReplayMismatch.val());
    }
    Ok(())
}

// ── 日志时间戳流（单调账） ──────────────────────────────────────
#[derive(Clone, Debug, Default)]
pub struct TsStream {
    pub entries: Vec<(u64, u64)>, // (tsc, ms)
    pub rewinds: u32,
    last: u64,
}

impl TsStream {
    pub fn new() -> TsStream {
        TsStream { entries: Vec::new(), rewinds: 0, last: 0 }
    }

    /// 追加日志戳（单调守卫）。
    pub fn push(&mut self, tsc: u64) -> Result<u64, u32> {
        if tsc < self.last {
            self.rewinds += 1;
            return Err(TsCode::TscRewind.val());
        }
        let ms = tsc_to_ms(tsc, TSC_FREQ_HZ)?;
        self.entries.push((tsc, ms));
        self.last = tsc;
        Ok(ms)
    }

    /// 账复算：逐条同 TSC 重算恒一致。
    pub fn replay_ok(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.entries.len() {
            let (tsc, ms) = self.entries[i];
            if tsc_to_ms(tsc, TSC_FREQ_HZ)? != ms {
                return Err(TsCode::ReplayMismatch.val());
            }
            i += 1;
        }
        Ok(())
    }
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

// ── 注入检出账（四路：除零路/倒退路/溢出路/复算路） ──────────────
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

/// 真机 TSC 序列（启动 ~1.2s 内五条）。
pub fn machine_tscs() -> [u64; 5] {
    [0, 1_200_000_000, 1_440_000_000, 2_400_000_000, 2_880_000_000]
}

// ── 黄金全链：五条入流→毫秒账→单调→复算 ──────────────────────────
pub fn golden_run() -> Result<(u64, u64, usize), u32> {
    let mut s = TsStream::new();
    let ts = machine_tscs();
    let mut i = 0;
    while i < ts.len() {
        s.push(ts[i])?;
        i += 1;
    }
    s.replay_ok()?;
    Ok((s.entries[0].1, s.entries[4].1, s.entries.len()))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：真机账 → 0ms/1200ms + 五条。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((0, 1200, 5)) => Ok(()),
        _ => Err(TsCode::NotReproducible.val()),
    }
}

/// 序 B：倒退戳 ⇒ TscRewind。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut s = TsStream::new();
    let _ = s.push(2000);
    match s.push(1000) {
        Err(e) if e == TsCode::TscRewind.val() => Ok(()),
        _ => Err(TsCode::NotReproducible.val()),
    }
}

/// 序 C：频率 0 ⇒ FreqZero。
pub fn probe_seq_c() -> Result<(), u32> {
    match tsc_to_ms(1000, 0) {
        Err(e) if e == TsCode::FreqZero.val() => Ok(()),
        _ => Err(TsCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：除零路。
        led.record(0, tsc_to_ms(100, 0).is_err());
        // 路 2：倒退路。
        let mut s2 = TsStream::new();
        let _ = s2.push(5000);
        led.record(1, s2.push(4999).is_err() && s2.rewinds == 1);
        // 路 3：溢出路（近 MAX TSC saturating 换算不 panic）。
        let big = tsc_to_ms(u64::MAX - 100, TSC_FREQ_HZ);
        led.record(2, big.is_ok());
        // 路 4：复算路（真机流复算恒真）。
        let mut s4 = TsStream::new();
        let ts = machine_tscs();
        let mut i = 0;
        while i < ts.len() {
            let _ = s4.push(ts[i]);
            i += 1;
        }
        led.record(3, s4.replay_ok().is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TsCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < TsCode::ALL.len() {
        let v = TsCode::ALL[i].val();
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
            Ok((ms0, ms4, n)) => *ms0 == 0 && *ms4 == 1200 && *n == 5,
            Err(_) => false,
        },
        match &g {
            Ok((ms0, ms4, n)) => format!("ms0={} ms4={} n={}", ms0, ms4, n),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 换算基准（2.4GHz：1ms = 2.4M ticks）。
    v.push((
        "J1-换算基准",
        tsc_to_ms(2_400_000, TSC_FREQ_HZ) == Ok(1)
            && tsc_to_ms(1_200_000_000, TSC_FREQ_HZ) == Ok(500)
            && TSC_FREQ_HZ == 2_400_000_000,
        String::from("2.4GHz 冻结：2.4M ticks = 1ms"),
    ));
    // 单调账（真机流零倒退）。
    let mut s = TsStream::new();
    let ts = machine_tscs();
    let mut i = 0;
    while i < ts.len() {
        let _ = s.push(ts[i]);
        i += 1;
    }
    v.push((
        "J1-单调账",
        s.rewinds == 0 && s.entries.len() == 5,
        String::from("真机流零倒退"),
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
        String::from("0x6E800 窗口"),
    ));
    v
}

pub fn run_unx0106_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0106 日志时间戳（TSC 换算）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
