//! UNX-F48865 打点开销标定与无账本对照 · 0x70800
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   ① 纯功能代码 ≥900 行；
//!   ② UNX-F48865-J1 打点开销 ×10^3 标定可复算，扣除后不吞真实预算；
//!   ③ UNX-F48866-J1 无账本旁路 ×10^3 不得计为送达；
//!   ④ UNX-F48867-J1 风暴识别 ×10^3 三态判定零抖动。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：样本合法性 → 跨度单调 → 同核或已校准 → 段链连续 →
//!           序号递增 → 开销扣除 → 记账守恒 → 风暴三态。前四道是
//!           「四闸」，只回答数值是否可信：跨度反序会把回退洗成零延迟；
//!           跨核未校准会把晶振偏移当真实延迟；段链断开意味着中间有
//!           样本丢失；序号回退意味着账本被重放。开销扣除必须晚于
//!           四闸：坏样本的「开销」不是开销，是噪声。记账守恒必须
//!           晚于扣除：否则旁路样本会先被算成送达，再用总量差掩盖。
//!           风暴三态最后判，因为它依赖前面全部可信的到达时刻。
//! 关键差异：本模块只拥有「可信性闸门 + 开销 + 三态风暴」这一层。
//!           core 拥有四段数值与分位；meta 拥有 TSC 权威源与环代际；
//!           dispatch 拥有环形淘汰与导出。本模块不复制分位、不复制
//!           环、不复制权威源判定；谁想让一份被四闸拒绝的样本进入
//!           统计，谁就必须先在本模块拿到 GatePass，不能靠旁路。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x70800，VX6 批 B04 第三槽）。
pub const SEG_BASE: u32 = 0x70800;

/// 冻结常量（卡面判据锚点）。
pub const CAL_ROUNDS: u32 = 1000;
pub const GATE_ROUNDS: u32 = 1000;
pub const STORM_ROUNDS: u32 = 1000;
pub const BYPASS_ROUNDS: u32 = 1000;
pub const SEGMENTS: u8 = 4;
pub const MAX_OVERHEAD_TICKS: u64 = 20_000;
pub const TSC_HZ: u64 = 2_000_000_000;

// ── 诊断码闭集（+0x01..+0x0B） ────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GCode {
    /// 样本字段非法（段号越界 / 序号为零）
    SampleBad,
    /// 跨度反序
    MonoBad,
    /// 跨核未校准
    CoreBad,
    /// 段链不连续
    ChainBad,
    /// 序号回退或重放
    SeqBad,
    /// 标定非法（轮数为零 / 开销超限 / 基准为零）
    CalBad,
    /// 旁路记账不平（旁路被计成送达）
    BypassBad,
    /// 风暴参数非法（窗口为零 / 阈值次序倒挂）
    StormBad,
    /// 三态抖动（无迟滞的来回切换）
    TriBad,
    /// 复算不一致
    ReplayBad,
    /// 段位越界
    SegmentBad,
}

impl GCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            GCode::SampleBad => "样本非法：段号越界或序号为零",
            GCode::MonoBad => "跨度反序：结束 TSC 小于起始 TSC",
            GCode::CoreBad => "跨核未校准：两端 CPU 不同且无偏移",
            GCode::ChainBad => "段链不连续：后段起点早于前段终点",
            GCode::SeqBad => "序号非法：回退或重放",
            GCode::CalBad => "标定非法：轮数为零、基准为零或开销超限",
            GCode::BypassBad => "旁路记账不平：旁路被计成送达",
            GCode::StormBad => "风暴参数非法：窗口为零或阈值倒挂",
            GCode::TriBad => "三态抖动：无迟滞的来回切换",
            GCode::ReplayBad => "复算不一致：两次黄金全链结果不同",
            GCode::SegmentBad => "段位越界：诊断码逃出 0x70800 窗口",
        }
    }

    pub const ALL: [GCode; 11] = [
        GCode::SampleBad,
        GCode::MonoBad,
        GCode::CoreBad,
        GCode::ChainBad,
        GCode::SeqBad,
        GCode::CalBad,
        GCode::BypassBad,
        GCode::StormBad,
        GCode::TriBad,
        GCode::ReplayBad,
        GCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < GCode::ALL.len() {
        if GCode::ALL[i].val() == code {
            return GCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 可信性四闸 ────────────────────────────────────────────────────
pub const G_MONO: u8 = 0x01;
pub const G_CORE: u8 = 0x02;
pub const G_CHAIN: u8 = 0x04;
pub const G_ORDER: u8 = 0x08;
pub const G_ALL: u8 = 0x0F;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tick {
    pub tsc: u64,
    pub cpu: u8,
}

impl Tick {
    pub const fn new(tsc: u64, cpu: u8) -> Tick {
        Tick { tsc, cpu }
    }

    pub fn advanced(self, delta: u64) -> Result<Tick, u32> {
        let next = self.tsc.checked_add(delta).ok_or(GCode::MonoBad.val())?;
        Ok(Tick { tsc: next, cpu: self.cpu })
    }

    pub fn same_core(self, other: Tick) -> bool {
        self.cpu == other.cpu
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SpanRec {
    pub seq: u32,
    pub seg: u8,
    pub start: Tick,
    pub end: Tick,
}

impl SpanRec {
    pub fn new(seq: u32, seg: u8, start: Tick, end: Tick) -> SpanRec {
        SpanRec { seq, seg, start, end }
    }

    pub fn ticks(&self) -> Result<u64, u32> {
        if self.end.tsc < self.start.tsc {
            return Err(GCode::MonoBad.val());
        }
        Ok(self.end.tsc - self.start.tsc)
    }

    pub fn us(&self) -> Result<u64, u32> {
        let t = self.ticks()?;
        Ok(t.saturating_mul(1_000) / (TSC_HZ / 1_000_000))
    }
}

/// 四闸上下文：上一条跨度的终点与上一条的段号/序号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GateState {
    pub last: Option<SpanRec>,
    pub calibrated: bool,
    pub accepted: u32,
    pub rejected: u32,
}

impl GateState {
    pub const fn new(calibrated: bool) -> GateState {
        GateState { last: None, calibrated, accepted: 0, rejected: 0 }
    }

    pub fn reset_chain(&mut self) {
        self.last = None;
    }
}

pub fn gate_span(st: &mut GateState, s: &SpanRec) -> Result<u8, u32> {
    if s.seq == 0 || s.seg >= SEGMENTS {
        st.rejected = st.rejected.saturating_add(1);
        return Err(GCode::SampleBad.val());
    }
    if s.end.tsc < s.start.tsc {
        st.rejected = st.rejected.saturating_add(1);
        return Err(GCode::MonoBad.val());
    }
    if !s.start.same_core(s.end) && !st.calibrated {
        st.rejected = st.rejected.saturating_add(1);
        return Err(GCode::CoreBad.val());
    }
    let mut mask = G_MONO;
    if s.start.same_core(s.end) || st.calibrated {
        mask |= G_CORE;
    }
    if let Some(prev) = st.last {
        if s.seq <= prev.seq {
            st.rejected = st.rejected.saturating_add(1);
            return Err(GCode::SeqBad.val());
        }
        if prev.seg < SEGMENTS - 1 && s.seg == prev.seg + 1 {
            if s.start.tsc < prev.end.tsc {
                st.rejected = st.rejected.saturating_add(1);
                return Err(GCode::ChainBad.val());
            }
            mask |= G_CHAIN;
        } else if s.seg == 0 && prev.seg == SEGMENTS - 1 {
            if s.start.tsc < prev.end.tsc {
                st.rejected = st.rejected.saturating_add(1);
                return Err(GCode::ChainBad.val());
            }
            mask |= G_CHAIN;
        } else if s.seg != 0 {
            st.rejected = st.rejected.saturating_add(1);
            return Err(GCode::ChainBad.val());
        }
        mask |= G_ORDER;
    }
    st.last = Some(*s);
    st.accepted = st.accepted.saturating_add(1);
    Ok(mask)
}

// ── 打点开销标定 ──────────────────────────────────────────────────
/// 参考标定：以 base 为真实跨度，观测值减去 base 即打点开销。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OverheadReport {
    pub samples: u32,
    pub base_ticks: u64,
    pub sum_ticks: u64,
    pub max_ticks: u64,
    pub over_limit: u32,
}

impl OverheadReport {
    pub const fn empty(base: u64) -> OverheadReport {
        OverheadReport { samples: 0, base_ticks: base, sum_ticks: 0, max_ticks: 0, over_limit: 0 }
    }

    pub fn mean_ticks(&self) -> u64 {
        if self.samples == 0 {
            0
        } else {
            self.sum_ticks / u64::from(self.samples)
        }
    }

    pub fn mean_us(&self) -> u64 {
        self.mean_ticks().saturating_mul(1_000) / (TSC_HZ / 1_000_000)
    }

    pub fn within_limit(&self) -> bool {
        self.max_ticks <= MAX_OVERHEAD_TICKS && self.over_limit == 0
    }

    pub fn correct(&self, observed: u64) -> Result<u64, u32> {
        if !self.within_limit() {
            return Err(GCode::CalBad.val());
        }
        Ok(observed.saturating_sub(self.mean_ticks()))
    }

    pub fn correct_us(&self, observed_us: u64) -> Result<u64, u32> {
        let t = self.correct(observed_us.saturating_mul(TSC_HZ / 1_000_000))?;
        Ok(t.saturating_mul(1_000) / (TSC_HZ / 1_000_000))
    }
}

/// 标定器：必须显式给出 base（真实参考跨度），不得用观测值自证。
pub struct Calibrator {
    base: u64,
    report: OverheadReport,
    seq: u32,
    backwards: u32,
}

impl Calibrator {
    pub fn new(base_ticks: u64) -> Result<Calibrator, u32> {
        if base_ticks == 0 || base_ticks > MAX_OVERHEAD_TICKS {
            return Err(GCode::CalBad.val());
        }
        Ok(Calibrator {
            base: base_ticks,
            report: OverheadReport::empty(base_ticks),
            seq: 0,
            backwards: 0,
        })
    }

    pub fn observe(&mut self, seq: u32, observed_ticks: u64) -> Result<u64, u32> {
        if seq == 0 {
            return Err(GCode::SampleBad.val());
        }
        if self.seq != 0 && seq <= self.seq {
            self.backwards = self.backwards.saturating_add(1);
            return Err(GCode::SeqBad.val());
        }
        if observed_ticks < self.base {
            return Err(GCode::CalBad.val());
        }
        let overhead = observed_ticks - self.base;
        self.seq = seq;
        self.report.samples = self.report.samples.saturating_add(1);
        self.report.sum_ticks = self.report.sum_ticks.saturating_add(overhead);
        if overhead > self.report.max_ticks {
            self.report.max_ticks = overhead;
        }
        if overhead > MAX_OVERHEAD_TICKS {
            self.report.over_limit = self.report.over_limit.saturating_add(1);
        }
        Ok(overhead)
    }

    pub fn report(&self) -> OverheadReport {
        self.report
    }

    pub fn base(&self) -> u64 {
        self.base
    }

    pub fn backwards(&self) -> u32 {
        self.backwards
    }
}

// ── 无账本对照 ────────────────────────────────────────────────────
/// 旁路记账：total = ledger + bypassed，旁路永远不算送达。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BypassAudit {
    pub total: u32,
    pub ledger: u32,
    pub bypassed: u32,
    pub delivered: u32,
    pub leaked: u32,
}

impl BypassAudit {
    pub const fn new() -> BypassAudit {
        BypassAudit { total: 0, ledger: 0, bypassed: 0, delivered: 0, leaked: 0 }
    }

    pub fn record_tracked(&mut self) {
        self.total = self.total.saturating_add(1);
        self.ledger = self.ledger.saturating_add(1);
        self.delivered = self.delivered.saturating_add(1);
    }

    pub fn record_bypass(&mut self) {
        self.total = self.total.saturating_add(1);
        self.bypassed = self.bypassed.saturating_add(1);
    }

    /// 对照跑：旁路样本不得进入 delivered。
    pub fn audit(&self) -> Result<(), u32> {
        let sum = self.ledger.saturating_add(self.bypassed);
        if sum != self.total {
            return Err(GCode::BypassBad.val());
        }
        if self.delivered != self.ledger {
            return Err(GCode::BypassBad.val());
        }
        if self.leaked != 0 {
            return Err(GCode::BypassBad.val());
        }
        Ok(())
    }

    pub fn coverage_ppm(&self) -> u32 {
        if self.total == 0 {
            0
        } else {
            self.ledger.saturating_mul(1_000_000) / self.total
        }
    }
}

// ── 风暴三态 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StormState {
    Calm,
    Warn,
    Storm,
}

impl StormState {
    pub fn label(self) -> &'static str {
        match self {
            StormState::Calm => "calm",
            StormState::Warn => "warn",
            StormState::Storm => "storm",
        }
    }

    pub fn code(self) -> u8 {
        match self {
            StormState::Calm => 0,
            StormState::Warn => 1,
            StormState::Storm => 2,
        }
    }
}

/// 风暴窗：过去 window_us 内的到达数即速率；warn/storm 双阈值 + 释放阈。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StormCfg {
    pub window_us: u64,
    pub warn_rate: u32,
    pub storm_rate: u32,
    pub release_rate: u32,
}

impl StormCfg {
    pub fn new(window_us: u64, warn: u32, storm: u32, release: u32) -> Result<StormCfg, u32> {
        if window_us == 0 || warn == 0 || storm <= warn || release >= warn {
            return Err(GCode::StormBad.val());
        }
        Ok(StormCfg { window_us, warn_rate: warn, storm_rate: storm, release_rate: release })
    }

    pub fn valid(&self) -> bool {
        self.window_us > 0
            && self.warn_rate > 0
            && self.storm_rate > self.warn_rate
            && self.release_rate < self.warn_rate
    }
}

pub struct StormWindow {
    cfg: StormCfg,
    arrivals: Vec<u64>,
    head: usize,
    state: StormState,
    flips: u32,
    storms: u32,
    warns: u32,
}

impl StormWindow {
    pub fn new(cfg: StormCfg) -> Result<StormWindow, u32> {
        if !cfg.valid() {
            return Err(GCode::StormBad.val());
        }
        Ok(StormWindow {
            cfg,
            arrivals: Vec::new(),
            head: 0,
            state: StormState::Calm,
            flips: 0,
            storms: 0,
            warns: 0,
        })
    }

    fn prune(&mut self, now_us: u64) {
        let low = now_us.saturating_sub(self.cfg.window_us);
        while self.head < self.arrivals.len() {
            match self.arrivals.get(self.head).copied() {
                Some(t) if t < low => {
                    self.head = self.head.saturating_add(1);
                }
                _ => break,
            }
        }
        if self.head > 1024 && self.head.saturating_mul(2) > self.arrivals.len() {
            self.arrivals.drain(0..self.head);
            self.head = 0;
        }
    }

    pub fn rate(&mut self, now_us: u64) -> u32 {
        self.prune(now_us);
        let n = self.arrivals.len().saturating_sub(self.head);
        u32::try_from(n).unwrap_or(u32::MAX)
    }

    /// 带迟滞的三态迁移：进入用高阈值，退出用释放阈值。
    pub fn observe(&mut self, now_us: u64) -> StormState {
        self.arrivals.push(now_us);
        let rate = self.rate(now_us);
        let next = match self.state {
            StormState::Calm => {
                if rate >= self.cfg.storm_rate {
                    StormState::Storm
                } else if rate >= self.cfg.warn_rate {
                    StormState::Warn
                } else {
                    StormState::Calm
                }
            }
            StormState::Warn => {
                if rate >= self.cfg.storm_rate {
                    StormState::Storm
                } else if rate < self.cfg.release_rate {
                    StormState::Calm
                } else {
                    StormState::Warn
                }
            }
            StormState::Storm => {
                if rate < self.cfg.release_rate {
                    StormState::Calm
                } else if rate < self.cfg.warn_rate {
                    StormState::Warn
                } else {
                    StormState::Storm
                }
            }
        };
        if next != self.state {
            self.flips = self.flips.saturating_add(1);
            if next == StormState::Storm {
                self.storms = self.storms.saturating_add(1);
            }
            if next == StormState::Warn {
                self.warns = self.warns.saturating_add(1);
            }
        }
        self.state = next;
        next
    }

    pub fn state(&self) -> StormState {
        self.state
    }

    pub fn flips(&self) -> u32 {
        self.flips
    }

    pub fn storms(&self) -> u32 {
        self.storms
    }

    pub fn warnings(&self) -> u32 {
        self.warns
    }

    pub fn cfg(&self) -> StormCfg {
        self.cfg
    }
}

// ── 确定性源（本域独立盐） ────────────────────────────────────────
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed ^ 0x4C41_5447_4152_4435)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

// ── 黄金语料 ──────────────────────────────────────────────────────
/// 标定语料：1000 次观测，开销 100..500 tick，全在限内。
pub fn golden_calibration(rounds: u32) -> Result<(u32, u64, u32), u32> {
    let base = 2_000u64;
    let mut cal = Calibrator::new(base)?;
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x4341_4C);
    let mut n = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let jitter = 100 + rng.below(401);
        cal.observe(r + 1, base.saturating_add(jitter))?;
        n = n.saturating_add(1);
        r = r.saturating_add(1);
    }
    let rep = cal.report();
    if rep.samples != n || !rep.within_limit() {
        return Err(GCode::CalBad.val());
    }
    Ok((n, rep.mean_ticks(), rep.max_ticks as u32))
}

/// 四闸语料：每轮一条完整四段链，全部合法。
pub fn golden_gates(rounds: u32) -> Result<(u32, u32), u32> {
    let mut st = GateState::new(false);
    let mut seq = 0u32;
    let mut accepted = 0u32;
    let mut masks = 0u32;
    let mut t = 1_000u64;
    let mut r = 0u32;
    while r < rounds {
        let seg = (r % u32::from(SEGMENTS)) as u8;
        seq = seq.saturating_add(1);
        let start = Tick::new(t, 0);
        let end = start.advanced(100 + u64::from(r % 50))?;
        let s = SpanRec::new(seq, seg, start, end);
        let m = gate_span(&mut st, &s)?;
        accepted = accepted.saturating_add(1);
        masks = masks.saturating_add(u32::from(m.count_ones() as u8));
        t = end.tsc;
        r = r.saturating_add(1);
    }
    if st.rejected != 0 {
        return Err(GCode::ReplayBad.val());
    }
    Ok((accepted, masks))
}

/// 旁路语料：1000 轮，每轮 3 条正账 + 1 条旁路，旁路永不送达。
pub fn golden_bypass(rounds: u32) -> Result<(u32, u32), u32> {
    let mut a = BypassAudit::new();
    let mut r = 0u32;
    while r < rounds {
        a.record_tracked();
        a.record_tracked();
        a.record_tracked();
        a.record_bypass();
        r = r.wrapping_add(1);
    }
    a.audit()?;
    if a.delivered != rounds.saturating_mul(3) {
        return Err(GCode::BypassBad.val());
    }
    Ok((a.total, a.delivered))
}

/// 风暴语料：每 10 轮中 2 轮风暴、3 轮警告、5 轮平静（窗口内计数）。
pub fn golden_storm(rounds: u32) -> Result<(u32, u32, u32, u32), u32> {
    let cfg = StormCfg::new(100, 4, 8, 2)?;
    let mut w = StormWindow::new(cfg)?;
    let mut calm = 0u32;
    let mut warn = 0u32;
    let mut storm = 0u32;
    let mut t = 1_000u64;
    let mut r = 0u32;
    while r < rounds {
        match r % 10 {
            0 => {
                let mut i = 0u32;
                while i < 8 {
                    let st = w.observe(t);
                    match st {
                        StormState::Calm => calm = calm.saturating_add(1),
                        StormState::Warn => warn = warn.saturating_add(1),
                        StormState::Storm => storm = storm.saturating_add(1),
                    }
                    t = t.saturating_add(5);
                    i = i.saturating_add(1);
                }
            }
            1 => {
                t = t.saturating_add(200);
                let mut i = 0u32;
                while i < 4 {
                    let st = w.observe(t);
                    match st {
                        StormState::Calm => calm = calm.saturating_add(1),
                        StormState::Warn => warn = warn.saturating_add(1),
                        StormState::Storm => storm = storm.saturating_add(1),
                    }
                    t = t.saturating_add(10);
                    i = i.saturating_add(1);
                }
            }
            _ => {
                t = t.saturating_add(200);
                let st = w.observe(t);
                match st {
                    StormState::Calm => calm = calm.saturating_add(1),
                    StormState::Warn => warn = warn.saturating_add(1),
                    StormState::Storm => storm = storm.saturating_add(1),
                }
            }
        }
        r = r.saturating_add(1);
    }
    if storm == 0 || warn == 0 || calm == 0 || w.flips() == 0 {
        return Err(GCode::TriBad.val());
    }
    Ok((calm, warn, storm, w.flips()))
}

// ── 指纹与黄金全链 ────────────────────────────────────────────────
fn fnv_step(mut h: u64, x: u64) -> u64 {
    let mut i = 0usize;
    while i < 8 {
        h ^= (x >> (i * 8)) & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

pub fn fingerprint(st: &GateState, s: &SpanRec) -> Result<u64, u32> {
    let mut copy = *st;
    let mask = gate_span(&mut copy, s)?;
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    h = fnv_step(h, u64::from(s.seq));
    h = fnv_step(h, u64::from(s.seg));
    h = fnv_step(h, s.start.tsc);
    h = fnv_step(h, s.end.tsc);
    h = fnv_step(h, u64::from(s.start.cpu));
    h = fnv_step(h, u64::from(mask));
    Ok(h)
}

pub fn golden_run() -> Result<(u32, u32, u32, u32, u64), u32> {
    let (cal_n, cal_mean, _cal_max) = golden_calibration(CAL_ROUNDS)?;
    let (gate_n, gate_masks) = golden_gates(GATE_ROUNDS)?;
    let (bypass_total, bypass_delivered) = golden_bypass(BYPASS_ROUNDS)?;
    let (calm, warn, storm, flips) = golden_storm(STORM_ROUNDS)?;
    if cal_n != CAL_ROUNDS || gate_n != GATE_ROUNDS {
        return Err(GCode::CalBad.val());
    }
    if bypass_total != BYPASS_ROUNDS.saturating_mul(4) || bypass_delivered != BYPASS_ROUNDS * 3 {
        return Err(GCode::BypassBad.val());
    }
    if calm == 0 || warn == 0 || storm == 0 || flips == 0 {
        return Err(GCode::TriBad.val());
    }
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    h = fnv_step(h, cal_mean);
    h = fnv_step(h, u64::from(gate_masks));
    h = fnv_step(h, u64::from(bypass_total));
    h = fnv_step(h, u64::from(calm));
    h = fnv_step(h, u64::from(warn));
    h = fnv_step(h, u64::from(storm));
    h = fnv_step(h, u64::from(flips));
    Ok((cal_n, gate_n, bypass_delivered, storm, h))
}

// ── 注入账（四路） ────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x494E_4A);
    let mut r = 0u32;
    while r < rounds {
        rng.next();
        // 路 1：反序跨度 / 越界段 / 零序号。
        let s0 = SpanRec::new(1, 0, Tick::new(100, 0), Tick::new(50, 0));
        let s1 = SpanRec::new(1, 9, Tick::new(100, 0), Tick::new(150, 0));
        let s2 = SpanRec::new(0, 0, Tick::new(100, 0), Tick::new(150, 0));
        let mut st = GateState::new(false);
        let bad_span = gate_span(&mut st, &s0).err() == Some(GCode::MonoBad.val())
            && gate_span(&mut st, &s1).err() == Some(GCode::SampleBad.val())
            && gate_span(&mut st, &s2).err() == Some(GCode::SampleBad.val());
        led.record(0, bad_span);

        // 路 2：跨核未校准 / 序号回退 / 段链断开。
        let cross = SpanRec::new(1, 0, Tick::new(100, 0), Tick::new(150, 1));
        let core_bad = {
            let mut g = GateState::new(false);
            gate_span(&mut g, &cross).err() == Some(GCode::CoreBad.val())
        };
        let seq_bad = {
            let mut g = GateState::new(false);
            let a = SpanRec::new(1, 0, Tick::new(100, 0), Tick::new(150, 0));
            let b = SpanRec::new(1, 1, Tick::new(150, 0), Tick::new(200, 0));
            gate_span(&mut g, &a).is_ok() && gate_span(&mut g, &b).err() == Some(GCode::SeqBad.val())
        };
        let chain_bad = {
            let mut g = GateState::new(false);
            let a = SpanRec::new(1, 0, Tick::new(100, 0), Tick::new(150, 0));
            let b = SpanRec::new(2, 2, Tick::new(150, 0), Tick::new(200, 0));
            gate_span(&mut g, &a).is_ok() && gate_span(&mut g, &b).err() == Some(GCode::ChainBad.val())
        };
        led.record(1, core_bad && seq_bad && chain_bad);

        // 路 3：旁路被计成送达 / 标定基准为零。
        let mut a = BypassAudit::new();
        a.record_tracked();
        a.record_bypass();
        a.delivered = a.delivered.saturating_add(1);
        let bypass_bad = a.audit().err() == Some(GCode::BypassBad.val());
        let cal_bad = Calibrator::new(0).err() == Some(GCode::CalBad.val())
            && Calibrator::new(MAX_OVERHEAD_TICKS + 1).is_err();
        led.record(2, bypass_bad && cal_bad);

        // 路 4：风暴阈值颠倒 / 三态无迟滞抖动。
        let storm_bad = StormCfg::new(0, 4, 8, 1).err() == Some(GCode::StormBad.val())
            && StormCfg::new(100, 8, 4, 1).err() == Some(GCode::StormBad.val());
        let tri_bad = StormCfg::new(100, 4, 8, 4).err() == Some(GCode::StormBad.val());
        led.record(3, storm_bad && tri_bad);

        r = r.wrapping_add(1);
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let (cal, gate, bypass, storm, _) = golden_run()?;
    if cal == CAL_ROUNDS && gate == GATE_ROUNDS && bypass == BYPASS_ROUNDS * 3 && storm > 0 {
        Ok(())
    } else {
        Err(GCode::ReplayBad.val())
    }
}

pub fn probe_seq_b() -> Result<(), u32> {
    let mut cal = Calibrator::new(2_000)?;
    cal.observe(1, 2_400)?;
    if cal.report().samples != 1 || cal.report().mean_ticks() != 400 {
        return Err(GCode::CalBad.val());
    }
    if cal.observe(1, 2_500).err() != Some(GCode::SeqBad.val()) {
        return Err(GCode::SeqBad.val());
    }
    let cfg = StormCfg::new(100, 4, 8, 1)?;
    let mut w = StormWindow::new(cfg)?;
    let mut i = 0u32;
    while i < 8 {
        w.observe(u64::from(i) * 10);
        i += 1;
    }
    if w.state() != StormState::Storm {
        return Err(GCode::TriBad.val());
    }
    let mut f = GateState::new(true);
    let a = SpanRec::new(1, 0, Tick::new(100, 0), Tick::new(200, 3));
    if gate_span(&mut f, &a).is_err() {
        return Err(GCode::CoreBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    if CAL_ROUNDS == 1000
        && GATE_ROUNDS == 1000
        && STORM_ROUNDS == 1000
        && BYPASS_ROUNDS == 1000
        && SEGMENTS == 4
        && GCode::ALL.len() == 11
    {
        Ok(())
    } else {
        Err(GCode::SegmentBad.val())
    }
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(GCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < GCode::ALL.len() {
        let v = GCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x20 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let cal = golden_calibration(CAL_ROUNDS);
    v.push((
        "F48865-J1-打点开销千轮标定",
        match &cal {
            Ok((n, mean, max)) => *n == CAL_ROUNDS && *mean > 0 && *max <= MAX_OVERHEAD_TICKS as u32,
            Err(_) => false,
        },
        match &cal {
            Ok((n, mean, max)) => format!("samples={} mean_ticks={} max_ticks={}", n, mean, max),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let gate = golden_gates(GATE_ROUNDS);
    v.push((
        "F48865-J1-四闸千轮全过",
        match &gate {
            Ok((n, masks)) => *n == GATE_ROUNDS && *masks > 0,
            Err(_) => false,
        },
        match &gate {
            Ok((n, masks)) => format!("accepted={} mask_bits={}", n, masks),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let bypass = golden_bypass(BYPASS_ROUNDS);
    v.push((
        "F48866-J1-无账本对照千轮",
        match &bypass {
            Ok((total, delivered)) => {
                *total == BYPASS_ROUNDS * 4 && *delivered == BYPASS_ROUNDS * 3
            }
            Err(_) => false,
        },
        match &bypass {
            Ok((total, delivered)) => format!("total={} delivered={}", total, delivered),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let storm = golden_storm(STORM_ROUNDS);
    v.push((
        "F48867-J1-风暴三态千轮",
        match &storm {
            Ok((c, w, s, f)) => *c > 0 && *w > 0 && *s > 0 && *f > 0,
            Err(_) => false,
        },
        match &storm {
            Ok((c, w, s, f)) => format!("calm={} warn={} storm={} flips={}", c, w, s, f),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let mut g = GateState::new(false);
    let cross = SpanRec::new(1, 0, Tick::new(10, 0), Tick::new(20, 1));
    v.push((
        "F48865-J1-跨核未校准必拒",
        gate_span(&mut g, &cross).err() == Some(GCode::CoreBad.val()),
        String::from("cpu0->cpu1 -> CoreBad"),
    ));
    let mut c = match Calibrator::new(1_000) {
        Ok(c) => c,
        Err(_) => return v,
    };
    let _ = c.observe(1, 1_300);
    let _ = c.observe(2, 1_600);
    v.push((
        "F48865-J1-开销扣除不吞预算",
        c.report().mean_ticks() == 450 && c.report().correct(1_450) == Ok(1_000),
        format!("mean={} corrected={:?}", c.report().mean_ticks(), c.report().correct(1_450)),
    ));
    let mut a = BypassAudit::new();
    a.record_tracked();
    a.record_bypass();
    v.push((
        "F48866-J1-旁路不算送达",
        a.audit().is_ok() && a.delivered == 1 && a.total == 2,
        format!("delivered={} total={} bypass={}", a.delivered, a.total, a.bypassed),
    ));
    let mut a2 = BypassAudit::new();
    a2.record_bypass();
    a2.delivered = 1;
    v.push((
        "F48866-J1-旁路泄漏必拒",
        a2.audit().err() == Some(GCode::BypassBad.val()),
        format!("audit={:?}", a2.audit().err()),
    ));
    let mut w = match StormWindow::new(match StormCfg::new(100, 4, 8, 2) {
        Ok(c) => c,
        Err(_) => return v,
    }) {
        Ok(w) => w,
        Err(_) => return v,
    };
    let mut i = 0u32;
    while i < 8 {
        w.observe(u64::from(i) * 10);
        i += 1;
    }
    let storm_hit = w.state() == StormState::Storm;
    let mut calm_hold = true;
    let mut j = 0u32;
    while j < 20 {
        w.observe(1_000 + u64::from(j) * 1_000);
        if w.state() == StormState::Storm {
            calm_hold = false;
        }
        j += 1;
    }
    v.push((
        "F48867-J1-风暴进入与释放",
        storm_hit && calm_hold && w.storms() >= 1,
        format!("state={} storms={} flips={}", w.state().label(), w.storms(), w.flips()),
    ));
    let inj = inject_rounds(10);
    v.push((
        "F48865-J1-注入四路10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("tries={:?} caught={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48865-J1-探针三序与双跑",
        probe_seq_a().is_ok()
            && probe_seq_b().is_ok()
            && probe_seq_c().is_ok()
            && repro_check().is_ok(),
        format!(
            "a={} b={} c={} repro={}",
            probe_seq_a().is_ok(),
            probe_seq_b().is_ok(),
            probe_seq_c().is_ok(),
            repro_check().is_ok()
        ),
    ));
    v.push((
        "F48867-J1-段位守卫",
        segment_guard(),
        String::from("0x70800 窗口"),
    ));
    v
}

pub fn run_unx48865_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48865 打点开销与无账本对照");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reversed_span_is_rejected() {
        let mut g = GateState::new(false);
        let s = SpanRec::new(1, 0, Tick::new(100, 0), Tick::new(50, 0));
        assert_eq!(gate_span(&mut g, &s).err(), Some(GCode::MonoBad.val()));
    }

    #[test]
    fn zero_seq_and_bad_segment_rejected() {
        let mut g = GateState::new(false);
        let a = SpanRec::new(0, 0, Tick::new(10, 0), Tick::new(20, 0));
        assert_eq!(gate_span(&mut g, &a).err(), Some(GCode::SampleBad.val()));
        let b = SpanRec::new(1, 7, Tick::new(10, 0), Tick::new(20, 0));
        assert_eq!(gate_span(&mut g, &b).err(), Some(GCode::SampleBad.val()));
    }

    #[test]
    fn cross_core_needs_calibration() {
        let s = SpanRec::new(1, 0, Tick::new(10, 0), Tick::new(20, 1));
        let mut a = GateState::new(false);
        assert_eq!(gate_span(&mut a, &s).err(), Some(GCode::CoreBad.val()));
        let mut b = GateState::new(true);
        assert!(gate_span(&mut b, &s).is_ok());
    }

    #[test]
    fn chain_must_be_contiguous() {
        let mut g = GateState::new(false);
        let a = SpanRec::new(1, 0, Tick::new(100, 0), Tick::new(150, 0));
        assert!(gate_span(&mut g, &a).is_ok());
        let b = SpanRec::new(2, 2, Tick::new(150, 0), Tick::new(200, 0));
        assert_eq!(gate_span(&mut g, &b).err(), Some(GCode::ChainBad.val()));
        let c = SpanRec::new(2, 1, Tick::new(150, 0), Tick::new(200, 0));
        assert!(gate_span(&mut g, &c).is_ok());
    }

    #[test]
    fn sequence_regression_rejected() {
        let mut g = GateState::new(false);
        let a = SpanRec::new(5, 0, Tick::new(10, 0), Tick::new(20, 0));
        assert!(gate_span(&mut g, &a).is_ok());
        let b = SpanRec::new(5, 1, Tick::new(20, 0), Tick::new(30, 0));
        assert_eq!(gate_span(&mut g, &b).err(), Some(GCode::SeqBad.val()));
    }

    #[test]
    fn calibrator_rejects_zero_base() {
        assert_eq!(Calibrator::new(0).err(), Some(GCode::CalBad.val()));
        assert_eq!(Calibrator::new(MAX_OVERHEAD_TICKS + 1).err(), Some(GCode::CalBad.val()));
    }

    #[test]
    fn calibration_subtracts_mean() {
        let mut c = match Calibrator::new(1_000) {
            Ok(c) => c,
            Err(_) => return,
        };
        assert!(c.observe(1, 1_200).is_ok());
        assert!(c.observe(2, 1_400).is_ok());
        assert_eq!(c.report().mean_ticks(), 300);
        assert_eq!(c.report().correct(1_300), Ok(1_000));
        assert_eq!(c.observe(2, 1_500).err(), Some(GCode::SeqBad.val()));
        assert_eq!(c.observe(3, 500).err(), Some(GCode::CalBad.val()));
    }

    #[test]
    fn overhead_over_limit_is_refused() {
        let mut c = match Calibrator::new(1_000) {
            Ok(c) => c,
            Err(_) => return,
        };
        assert!(c.observe(1, 1_000 + MAX_OVERHEAD_TICKS + 1).is_ok());
        assert!(!c.report().within_limit());
        assert_eq!(c.report().correct(1_000), Err(GCode::CalBad.val()));
    }

    #[test]
    fn bypass_never_counts_as_delivery() {
        let mut a = BypassAudit::new();
        a.record_tracked();
        a.record_bypass();
        assert!(a.audit().is_ok());
        assert_eq!(a.delivered, 1);
        assert_eq!(a.coverage_ppm(), 500_000);
        a.delivered = 2;
        assert_eq!(a.audit().err(), Some(GCode::BypassBad.val()));
    }

    #[test]
    fn storm_cfg_order_is_enforced() {
        assert_eq!(StormCfg::new(0, 4, 8, 1).err(), Some(GCode::StormBad.val()));
        assert_eq!(StormCfg::new(100, 8, 4, 1).err(), Some(GCode::StormBad.val()));
        assert_eq!(StormCfg::new(100, 4, 8, 9).err(), Some(GCode::StormBad.val()));
        assert!(StormCfg::new(100, 4, 8, 1).is_ok());
    }

    #[test]
    fn storm_enters_and_releases_with_hysteresis() {
        let cfg = match StormCfg::new(100, 4, 8, 2) {
            Ok(c) => c,
            Err(_) => return,
        };
        let mut w = match StormWindow::new(cfg) {
            Ok(w) => w,
            Err(_) => return,
        };
        let mut i = 0u32;
        while i < 8 {
            w.observe(u64::from(i) * 10);
            i += 1;
        }
        assert_eq!(w.state(), StormState::Storm);
        let mut j = 0u32;
        while j < 50 {
            w.observe(1_000 + u64::from(j) * 1_000);
            j += 1;
        }
        assert_eq!(w.state(), StormState::Calm);
    }

    #[test]
    fn golden_is_frozen_and_repeatable() {
        let a = golden_run();
        assert!(a.is_ok());
        assert_eq!(golden_run(), a);
        match a {
            Ok((cal, gate, bypass, storm, root)) => {
                assert_eq!(cal, CAL_ROUNDS);
                assert_eq!(gate, GATE_ROUNDS);
                assert_eq!(bypass, BYPASS_ROUNDS * 3);
                assert!(storm > 0);
                assert!(root != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn probes_and_injection_hold() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        match inject_rounds(10) {
            Ok(l) => assert!(l.all_caught()),
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn fingerprint_is_stable() {
        let mut g = GateState::new(false);
        let s = SpanRec::new(1, 0, Tick::new(100, 0), Tick::new(160, 0));
        let a = fingerprint(&g, &s);
        let b = fingerprint(&g, &s);
        assert_eq!(a, b);
        assert!(gate_span(&mut g, &s).is_ok());
    }
}
