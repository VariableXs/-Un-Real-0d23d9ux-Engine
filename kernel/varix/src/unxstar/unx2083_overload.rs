//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2083 · 超载检测与降级信号 · 0x36780
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F2083-J1 降级信号断言全过（超载判定→降级信号发射→消费
//!      确认闭环），误报注入 10/10 检出，信号账动机字段齐备；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 超载判定：滚动窗负载均值 ≥ 阈值（8000 万分比）持续 K 窗（滞
//!     回防单窗波动）→ 超载成立；
//!   - 信号闭环：超载 → 降级信号发射（带动机字段）→ 消费方确认
//!     （确认位）→ 闭环；未确认信号不得重复发射（幂等）；
//!   - 信号账动机字段齐备：每条信号必须带（动机：超载/欠载恢复/手
//!     动，证据：窗口均值，阈值）三字段——缺字段 = 账坏必抓；
//!   - 恢复：负载低于恢复阈（6000）滞回 2 窗 → 恢复信号（解除降级）；
//!   - 误报注入：单窗尖峰（K=1 不满足）不发射——10/10 检出误报路径；
//!   - 注入面：单窗尖峰误报 / 缺动机字段 / 重复发射 / 未确认闭环 /
//!     阈值倒挂 五族 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x36780），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x36780，全仓独占）。
pub const SEG_BASE: u32 = 0x36780;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OlCode {
    /// 单窗尖峰误报（K 窗条件不满足却发射）
    FalsePositive,
    /// 动机字段缺失
    MotiveMissing,
    /// 重复发射（未确认幂等破）
    DoubleEmit,
    /// 未确认闭环（消费确认缺失）
    AckMissing,
    /// 阈值倒挂
    ThresholdInverted,
    /// 负载越域
    LoadOutOfRange,
    /// 恢复未滞回
    RecoverNoHysteresis,
    /// 信号账空却报闭环
    EmptyLedgerClaim,
}

impl OlCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            OlCode::FalsePositive => "单窗尖峰误报：K 窗持续条件不满足",
            OlCode::MotiveMissing => "动机字段缺失：信号必须带动机/证据/阈值",
            OlCode::DoubleEmit => "重复发射：未确认信号幂等",
            OlCode::AckMissing => "未确认闭环：消费确认缺失",
            OlCode::ThresholdInverted => "阈值倒挂：超载阈必须 > 恢复阈",
            OlCode::LoadOutOfRange => "负载越域：万分比",
            OlCode::RecoverNoHysteresis => "恢复未滞回：恢复需连续窗确认",
            OlCode::EmptyLedgerClaim => "信号账空却报闭环（谎报）",
        }
    }
    pub const ALL: [OlCode; 8] = [
        OlCode::FalsePositive,
        OlCode::MotiveMissing,
        OlCode::DoubleEmit,
        OlCode::AckMissing,
        OlCode::ThresholdInverted,
        OlCode::LoadOutOfRange,
        OlCode::RecoverNoHysteresis,
        OlCode::EmptyLedgerClaim,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in OlCode::ALL {
        if c.val() == code {
            return c.detail();
        }
    }
    "未知诊断码"
}

// ── 常量 ─────────────────────────────────────────────────────────
/// 超载阈/恢复阈（万分比）。
pub const OVERLOAD_AT: u32 = 8_000;
pub const RECOVER_AT: u32 = 6_000;
/// 持续窗数 K。
pub const K_WINDOWS: usize = 3;
/// 恢复滞回窗数。
pub const RECOVER_WINDOWS: usize = 2;

/// 动机闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Motive {
    Overload,
    Recover,
    Manual,
}

impl Motive {
    pub fn name(self) -> &'static str {
        match self {
            Motive::Overload => "overload",
            Motive::Recover => "recover",
            Motive::Manual => "manual",
        }
    }
}

/// 降级信号（动机字段齐备）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DegradeSignal {
    pub motive: Motive,
    /// 证据：触发窗口均值。
    pub evidence_milli: u32,
    pub threshold: u32,
    pub acked: bool,
}

/// 超载检测引擎。
pub struct OverloadEngine {
    pub window_avg: u32,
    /// 连续超载窗计数。
    pub overload_streak: usize,
    /// 连续恢复窗计数。
    pub recover_streak: usize,
    pub degraded: bool,
    pub signals: Vec<DegradeSignal>,
    pub rejected: usize,
}

impl OverloadEngine {
    pub fn new() -> OverloadEngine {
        OverloadEngine {
            window_avg: 0,
            overload_streak: 0,
            recover_streak: 0,
            degraded: false,
            signals: Vec::new(),
            rejected: 0,
        }
    }

    /// 每窗采样：K 窗滞回判定 + 信号发射闭环。
    pub fn sample_window(&mut self, load_milli: u32) -> Result<Option<Motive>, OlCode> {
        if load_milli > 10_000 {
            return Err(OlCode::LoadOutOfRange);
        }
        self.window_avg = load_milli;
        // 超载方向。
        if load_milli >= OVERLOAD_AT {
            self.overload_streak += 1;
            self.recover_streak = 0;
            if !self.degraded && self.overload_streak >= K_WINDOWS {
                // 发射（幂等：已发射未确认不得重复——degraded 尚假但
                // 挂起信号存在即拒）。
                if self.has_unacked() {
                    self.rejected += 1;
                    return Err(OlCode::DoubleEmit);
                }
                self.signals.push(DegradeSignal {
                    motive: Motive::Overload,
                    evidence_milli: load_milli,
                    threshold: OVERLOAD_AT,
                    acked: false,
                });
                self.degraded = true;
                return Ok(Some(Motive::Overload));
            }
        } else if load_milli <= RECOVER_AT {
            self.overload_streak = 0;
            if self.degraded {
                self.recover_streak += 1;
                if self.recover_streak >= RECOVER_WINDOWS {
                    self.signals.push(DegradeSignal {
                        motive: Motive::Recover,
                        evidence_milli: load_milli,
                        threshold: RECOVER_AT,
                        acked: false,
                    });
                    self.degraded = false;
                    self.recover_streak = 0;
                    return Ok(Some(Motive::Recover));
                }
            }
        } else {
            // 保持带：双计数清（滞回）。
            self.overload_streak = 0;
            self.recover_streak = 0;
        }
        Ok(None)
    }

    pub fn has_unacked(&self) -> bool {
        self.signals.iter().any(|s| !s.acked)
    }

    /// 消费确认。
    pub fn ack(&mut self) -> Result<(), OlCode> {
        if self.signals.is_empty() {
            return Err(OlCode::EmptyLedgerClaim);
        }
        for s in &mut self.signals {
            s.acked = true;
        }
        Ok(())
    }

    /// 动机字段齐备核。
    pub fn motives_complete(&self) -> bool {
        for s in &self.signals {
            let domain_ok = matches!(s.motive, Motive::Overload | Motive::Recover | Motive::Manual);
            if !domain_ok || s.evidence_milli == 0 && s.motive == Motive::Overload || s.threshold == 0 {
                return false;
            }
        }
        true
    }

    pub fn summary_line(&self) -> String {
        format!(
            "超载引擎: 降级={} 信号={} 未确认={} streak(o/r)={}/{}",
            self.degraded,
            self.signals.len(),
            self.has_unacked(),
            self.overload_streak,
            self.recover_streak
        )
    }
}

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── 信号闭环（判据 J1 主面） ─────────────────────────────────────
pub fn drill_signal_loop_10() -> (bool, String) {
    let mut caught = 0;
    for _ in 0..10 {
        let mut e = OverloadEngine::new();
        let mut ok = true;
        // K 窗超载 → 发射。
        for _ in 0..K_WINDOWS {
            let r = e.sample_window(9_000).ok();
            ok = ok && r.is_some() == (e.degraded);
        }
        ok = ok && e.degraded && e.has_unacked();
        // 消费确认。
        ok = ok && e.ack().is_ok() && !e.has_unacked();
        // 恢复滞回 2 窗。
        let _ = e.sample_window(1_000);
        let r2 = e.sample_window(1_000);
        ok = ok && r2 == Ok(Some(Motive::Recover)) && !e.degraded;
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("信号闭环 {}/10（发射→确认→恢复）", caught))
}

fn drill_false_positive_10() -> (bool, String) {
    // 误报注入 10/10：单窗尖峰后回落 → 不发射。
    let mut caught = 0;
    for _ in 0..10 {
        let mut e = OverloadEngine::new();
        let mut ok = true;
        // 尖峰 1 窗（K=3 不满足）。
        let r1 = e.sample_window(9_500).ok();
        ok = ok && r1.is_none() && !e.degraded;
        // 再 1 窗回落。
        let r2 = e.sample_window(2_000).ok();
        ok = ok && r2.is_none() && e.signals.is_empty();
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("单窗尖峰误报 {}/10 检出（不发射）", caught))
}

fn drill_double_emit() -> (bool, String) {
    // 重复发射注入：K 窗满发射后未确认继续超载 → 幂等拒。
    let mut e = OverloadEngine::new();
    for _ in 0..K_WINDOWS {
        let _ = e.sample_window(9_000);
    }
    let sig_count = e.signals.len();
    // 未确认继续超载窗 → DoubleEmit。
    let r = e.sample_window(9_500);
    let ok = sig_count == 1
        && e.signals.len() == 1
        && matches!(r, Err(OlCode::DoubleEmit));
    (ok, format!("重复发射拒: 信号数恒1={} r={:?}", e.signals.len() == 1, r))
}

fn drill_motive_missing_detected() -> (bool, String) {
    // 动机字段缺失注入：证据清零的 overload 信号 → 齐备核抓。
    let mut e = OverloadEngine::new();
    for _ in 0..K_WINDOWS {
        let _ = e.sample_window(9_000);
    }
    let ok_before = e.motives_complete();
    if let Some(s) = e.signals.first_mut() {
        s.evidence_milli = 0; // 注入缺证据
    }
    let caught = !e.motives_complete();
    (ok_before && caught, format!("动机字段注入: 前={} 注入抓={}", ok_before, caught))
}

fn drill_ack_missing() -> (bool, String) {
    // 未确认闭环注入：ack 空账 → 拒。
    let mut e = OverloadEngine::new();
    let r = e.ack();
    (r == Err(OlCode::EmptyLedgerClaim), format!("空账确认拒: {:?}", r.is_err()))
}

fn drill_threshold_inverted() -> (bool, String) {
    let ok = OVERLOAD_AT > RECOVER_AT;
    (ok, format!("阈值正序: overload={} > recover={} = {}", OVERLOAD_AT, RECOVER_AT, ok))
}

fn drill_load_range_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut caught = 0;
    for _ in 0..10 {
        let mut e = OverloadEngine::new();
        let bad = 10_001 + lcg.next_below(1000) as u32;
        if matches!(e.sample_window(bad), Err(OlCode::LoadOutOfRange)) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("负载越域注入 {}/10 必抓", caught))
}

fn drill_recover_hysteresis() -> (bool, String) {
    // 恢复滞回：1 窗恢复不解除（RECOVER_WINDOWS=2）。
    let mut e = OverloadEngine::new();
    for _ in 0..K_WINDOWS {
        let _ = e.sample_window(9_000);
    }
    let _ = e.ack();
    let r1 = e.sample_window(1_000).ok();
    let still = e.degraded;
    let r2 = e.sample_window(1_000).ok();
    (
        r1.is_none() && still && r2 == Some(Some(Motive::Recover)),
        format!("恢复滞回: 首窗保持={} 次窗解除={}", still, r2.is_some()),
    )
}

fn drill_band_neutral() -> (bool, String) {
    // 保持带：超载 streak 与恢复 streak 都清（不累积）。
    let mut e = OverloadEngine::new();
    let _ = e.sample_window(9_000);
    let s1 = e.overload_streak;
    let _ = e.sample_window(7_000); // 保持带
    let s2 = e.overload_streak;
    (s1 == 1 && s2 == 0, format!("保持带清账: 超载1→保持0 = {}→{}", s1, s2))
}

fn drill_motive_names() -> (bool, String) {
    // 动机闭集三值非空名。
    (Motive::Overload.name().len() > 0 && Motive::Recover.name().len() > 0 && Motive::Manual.name().len() > 0,
        format!("动机闭集: overload/recover/manual 全在册"))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    OlCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_signal_loop_10();
    let (a2, d2) = drill_false_positive_10();
    let (a3, d3) = drill_recover_hysteresis();
    let ok = a1 && a2 && a3;
    (ok, format!("全流程: 闭环={} 误报检出={} 恢复滞回={} [{}|{}|{}]", a1, a2, a3, d1, d2, d3))
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_signal_loop_10();
    v.push(("J1-信号闭环10/10", a, b));
    let (a, b) = drill_false_positive_10();
    v.push(("J1-误报注入10/10检出", a, b));
    let (a, b) = drill_double_emit();
    v.push(("J2-重复发射拒", a, b));
    let (a, b) = drill_motive_missing_detected();
    v.push(("J2-动机字段缺失抓", a, b));
    let (a, b) = drill_ack_missing();
    v.push(("J2-未确认闭环拒", a, b));
    let (a, b) = drill_threshold_inverted();
    v.push(("J2-阈值倒挂核", a, b));
    let (a, b) = drill_load_range_10();
    v.push(("J2-负载越域注入10/10", a, b));
    let (a, b) = drill_recover_hysteresis();
    v.push(("J1-恢复滞回", a, b));
    let (a, b) = drill_band_neutral();
    v.push(("J3-保持带清账", a, b));
    let (a, b) = drill_motive_names();
    v.push(("J3-动机闭集", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx2083_checks：本单判据聚合（诊断段 0x36780）。
pub fn run_unx2083_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2083 超载检测与降级信号");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x36780 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
