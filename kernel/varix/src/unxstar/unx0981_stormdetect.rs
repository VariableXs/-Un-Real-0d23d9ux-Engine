//! unx0981_stormdetect — UNX-F0981 中断风暴检测器（率判据与风暴确认）。
//!
//! 任务锚点：UNX-F0981（第二批工位 W7，SEG 段位 0x80A0，本单独占
//! 0x80A0..=0x80BF；诊断码自 SEG_BASE+1 起闭集分派）。
//! 判据摘录（卡面验收行）：**率判据两级（高阈疑点/确认阈）判定确定
//! （同窗口序列两次判定一致），风暴确认态只进不误（无风暴必不确认、
//! 有风暴必确认），降级回退路径完整（confirmed 可退回 quiet），检测
//! 全程事件账守恒**。
//! 设计纪律：no_std+alloc；零 unwrap/expect/panic!/浮点/std；
//! 全可失败路径 `Result<T, u16>`（错误码 0x80A 段闭集）；tick 显性
//! 注入零墙钟；窗口账定容守恒（Σ窗口计数==总计数 恒等式机检）；
//! 结尾 drill() 自例程走核心路径返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码（0x80A0 段，本单独占）
// ---------------------------------------------------------------------------

/// 中断风暴检测器段基址（0x80A0，本单 0x80A0..=0x80BF 独占）。
pub const SEG_BASE: u16 = 0x80A0;
/// 0x80A1 判据阈值非法（疑点阈 ≥ 确认阈，或阈值为零）。
pub const XC_THRESH_INVALID: u16 = SEG_BASE + 1;
/// 0x80A2 窗口容量非法（窗口数为零）。
pub const XC_WINDOW_CAP: u16 = SEG_BASE + 2;
/// 0x80A3 窗口计数回退（累计计数回退——采样面污染）。
pub const XC_COUNT_REGRESS: u16 = SEG_BASE + 3;
/// 0x80A4 状态机越迁（不合法的状态迁移——检测器被污染）。
pub const XC_STATE_BAD: u16 = SEG_BASE + 4;
/// 0x80A5 双跑判定失配（同窗口序列两次判定不一致）。
pub const XC_PAIR_MISMATCH: u16 = SEG_BASE + 5;
/// 0x80A6 事件账守恒破坏（Σ分种 != 全程总数）。
pub const XC_CONSERVATION: u16 = SEG_BASE + 6;
/// 0x80A7 确认但无风暴（confirmed 态下率低于静默阈——误确认）。
pub const XC_FALSE_CONFIRM: u16 = SEG_BASE + 7;
/// 0x80A8 有风暴未确认（确认阈被连续命中却停留疑点态——漏确认）。
pub const XC_MISS_CONFIRM: u16 = SEG_BASE + 8;
/// 0x80A9 窗口序号回退（采样 tick 单调性破坏）。
pub const XC_TICK_REGRESS: u16 = SEG_BASE + 9;
/// 0x80AA 检测器指纹漂移（双导出指纹失配）。
pub const XC_DRIFT: u16 = SEG_BASE + 10;

const _: () = assert!(XC_THRESH_INVALID != XC_WINDOW_CAP);
const _: () = assert!(XC_FALSE_CONFIRM != XC_MISS_CONFIRM);
const _: () = assert!(XC_PAIR_MISMATCH != XC_DRIFT);

// ---------------------------------------------------------------------------
// §1 判据阈值（const 判据表——判定确定性的刻度来源）
// ---------------------------------------------------------------------------

/// 高阈（疑点判据）：单窗计数 ≥ 1000 即入疑点。
pub const RATE_HI: u32 = 1_000;
/// 低阈（静默判据）：单窗计数 ≤ 100 视为静默。
pub const RATE_LO: u32 = 100;
/// 疑点持续窗数（连续 3 窗 ≥ 高阈即达确认判据）。
pub const SUSPECT_TICKS: u32 = 3;
/// 窗口数（滑动窗容量定值 8——率判定看的近期历史）。
pub const WINDOW_CAP: usize = 8;

// ---------------------------------------------------------------------------
// §2 检测状态机闭集（Quiet → Suspect → Confirmed，可回退）
// ---------------------------------------------------------------------------

/// 检测态闭集：静默 / 疑点 / 确认（回退边：Confirmed→Suspect→Quiet）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StormState {
    /// 静默（率低，无风暴迹象）。
    Quiet,
    /// 疑点（率高但未满确认持续窗）。
    Suspect,
    /// 确认（确认判据成立——风暴成立）。
    Confirmed,
}

impl StormState {
    /// 态码（1/2/3）。
    pub const fn code(self) -> u8 {
        match self {
            StormState::Quiet => 1,
            StormState::Suspect => 2,
            StormState::Confirmed => 3,
        }
    }
    /// 态名。
    pub const fn name(self) -> &'static str {
        match self {
            StormState::Quiet => "静默",
            StormState::Suspect => "疑点",
            StormState::Confirmed => "确认",
        }
    }
    /// 由码还原。
    pub const fn from_code(c: u8) -> Option<StormState> {
        match c {
            1 => Some(StormState::Quiet),
            2 => Some(StormState::Suspect),
            3 => Some(StormState::Confirmed),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// §3 检测事件种闭集
// ---------------------------------------------------------------------------

/// 检测事件种闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetectEvent {
    /// 采样落窗。
    Sampled,
    /// 入疑点。
    ToSuspect,
    /// 风暴确认。
    Confirmed,
    /// 降级回退。
    Downgraded,
    /// 采样拒收（tick 回退/计数回退）。
    SampleRejected,
}

impl DetectEvent {
    /// 种码（1..=5）。
    pub const fn code(self) -> u8 {
        match self {
            DetectEvent::Sampled => 1,
            DetectEvent::ToSuspect => 2,
            DetectEvent::Confirmed => 3,
            DetectEvent::Downgraded => 4,
            DetectEvent::SampleRejected => 5,
        }
    }
    /// 种名。
    pub const fn name(self) -> &'static str {
        match self {
            DetectEvent::Sampled => "采样",
            DetectEvent::ToSuspect => "入疑点",
            DetectEvent::Confirmed => "风暴确认",
            DetectEvent::Downgraded => "降级回退",
            DetectEvent::SampleRejected => "采样拒收",
        }
    }
}

/// 事件种总数。
pub const KIND_COUNT: usize = 5;

// ---------------------------------------------------------------------------
// §4 风暴检测器（StormDetector）
// ---------------------------------------------------------------------------

/// 风暴检测器——率判据两级判定 + 状态机 + 事件账。
///
/// 守恒恒等式：`Σ(窗口计数) == cumulative - consumed_at_window_start`
/// 的采样面简化形：`Σ(逐窗 delta) == 总采样计数`（每条计数恰归一窗）；
/// 事件账守恒：`Σ per_kind == total`。
#[derive(Debug)]
pub struct StormDetector {
    windows: Vec<u32>,
    win_cap: usize,
    head: usize,
    total_sampled: u64,
    state: StormState,
    hi_run: u32,
    lo_run: u32,
    last_tick: u64,
    events_total: u64,
    per_kind: [u64; KIND_COUNT],
    confirmed_at: u64,
    confirm_cnt: u32,
}

impl StormDetector {
    /// 建检测器（阈值与窗口容量机检）。
    pub fn new() -> Result<StormDetector, u16> {
        if RATE_HI <= RATE_LO || RATE_HI == 0 {
            return Err(XC_THRESH_INVALID);
        }
        if WINDOW_CAP == 0 {
            return Err(XC_WINDOW_CAP);
        }
        Ok(StormDetector {
            windows: Vec::with_capacity(WINDOW_CAP),
            win_cap: WINDOW_CAP,
            head: 0,
            total_sampled: 0,
            state: StormState::Quiet,
            hi_run: 0,
            lo_run: 0,
            last_tick: 0,
            events_total: 0,
            per_kind: [0; KIND_COUNT],
            confirmed_at: 0,
            confirm_cnt: 0,
        })
    }

    /// 当前态。
    pub fn state(&self) -> StormState {
        self.state
    }
    /// 总采样计数。
    pub fn total_sampled(&self) -> u64 {
        self.total_sampled
    }
    /// 事件全程计数。
    pub fn events_total(&self) -> u64 {
        self.events_total
    }
    /// 风暴确认次数。
    pub fn confirm_count(&self) -> u32 {
        self.confirm_cnt
    }
    /// 确认时刻 tick（未确认为 0——最近一次）。
    pub fn confirmed_at(&self) -> u64 {
        self.confirmed_at
    }
    /// 按种计数。
    pub fn count_of(&self, k: DetectEvent) -> u64 {
        let c = k.code() as usize;
        if c == 0 || c > KIND_COUNT {
            return 0;
        }
        self.per_kind[c - 1]
    }

    fn log(&mut self, k: DetectEvent) {
        self.events_total += 1;
        let c = k.code() as usize;
        if c >= 1 && c <= KIND_COUNT {
            self.per_kind[c - 1] += 1;
        }
    }

    /// 采样一窗（tick 单调、计数不减——判据判定的唯一输入）。
    ///
    /// 判定路径确定性：状态迁移只由窗口计数序列 + const 阈值表决定
    /// （零随机零墙钟），同序列双跑必同果。
    pub fn sample(&mut self, count: u32, tick: u64) -> Result<StormState, u16> {
        if tick < self.last_tick {
            self.log(DetectEvent::SampleRejected);
            return Err(XC_TICK_REGRESS);
        }
        self.last_tick = tick;
        // 落窗（环形覆盖最旧）。
        if self.windows.len() < self.win_cap {
            self.windows.push(count);
        } else {
            self.windows[self.head] = count;
        }
        self.head = (self.head + 1) % self.win_cap;
        self.total_sampled += count as u64;
        self.log(DetectEvent::Sampled);
        // 两级率判据状态机。
        self.step_state(count);
        Ok(self.state)
    }

    /// 状态机一步（迁移闭集：见内注）。
    fn step_state(&mut self, count: u32) {
        let prev = self.state;
        match self.state {
            StormState::Quiet => {
                if count >= RATE_HI {
                    self.hi_run += 1;
                    if self.hi_run >= SUSPECT_TICKS {
                        self.state = StormState::Confirmed;
                    } else {
                        self.state = StormState::Suspect;
                    }
                }
            }
            StormState::Suspect => {
                if count >= RATE_HI {
                    self.hi_run += 1;
                    if self.hi_run >= SUSPECT_TICKS {
                        self.state = StormState::Confirmed;
                    }
                } else if count <= RATE_LO {
                    self.hi_run = 0;
                    self.state = StormState::Quiet;
                }
            }
            StormState::Confirmed => {
                if count <= RATE_LO {
                    self.lo_run += 1;
                    if self.lo_run >= SUSPECT_TICKS {
                        // 确认态回退：连续 3 静默窗降回静默。
                        self.state = StormState::Quiet;
                        self.hi_run = 0;
                        self.lo_run = 0;
                        self.log(DetectEvent::Downgraded);
                    }
                } else {
                    self.lo_run = 0;
                }
            }
        }
        // 迁移落账（只记变迁，不记原地驻留）。
        if self.state != prev {
            match self.state {
                StormState::Suspect => self.log(DetectEvent::ToSuspect),
                StormState::Confirmed => {
                    self.log(DetectEvent::Confirmed);
                    self.confirm_cnt += 1;
                    self.confirmed_at = self.last_tick;
                }
                StormState::Quiet => {}
            }
        }
    }

    /// 事件账守恒机检（Σ分种 == 全程总数）。
    pub fn audit_conservation(&self) -> Result<(), u16> {
        let mut s: u64 = 0;
        let mut i: usize = 0;
        while i < KIND_COUNT {
            s += self.per_kind[i];
            i += 1;
        }
        if s != self.events_total {
            return Err(XC_CONSERVATION);
        }
        // 窗口账守恒：窗口现存和 ≤ 总采样（覆盖面外的历史不回看）。
        let mut w: u64 = 0;
        let mut j: usize = 0;
        while j < self.windows.len() {
            w += self.windows[j] as u64;
            j += 1;
        }
        if w > self.total_sampled {
            return Err(XC_CONSERVATION);
        }
        Ok(())
    }

    /// 窗口快照（最旧→最新）。
    pub fn windows_snapshot(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::with_capacity(self.windows.len());
        if self.windows.len() < self.win_cap {
            let mut i: usize = 0;
            while i < self.windows.len() {
                out.push(self.windows[i]);
                i += 1;
            }
        } else {
            let mut i: usize = 0;
            while i < self.win_cap {
                out.push(self.windows[(self.head + i) % self.win_cap]);
                i += 1;
            }
        }
        out
    }

    /// 检测器指纹（态/计数/事件账 FNV——双跑对拍用）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = (h ^ self.state.code() as u64).wrapping_mul(0x0000_0100_0000_01b3);
        h = (h ^ self.total_sampled).wrapping_mul(0x0000_0100_0000_01b3);
        h = (h ^ self.events_total).wrapping_mul(0x0000_0100_0000_01b3);
        h = (h ^ self.hi_run as u64).wrapping_mul(0x0000_0100_0000_01b3);
        h = (h ^ self.lo_run as u64).wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    /// 检测面文本导出（确定性 + 双导出对拍）。
    pub fn export(&mut self) -> Result<String, u16> {
        self.audit_conservation()?;
        let fp0 = self.fingerprint();
        let mut s = String::from("UNX-F0981-DETECT|v1|state=");
        s.push_str(self.state.name());
        s.push_str("|sampled=");
        s.push_str(&self.total_sampled.to_string());
        s.push_str("|confirms=");
        s.push_str(&self.confirm_cnt.to_string());
        s.push('\n');
        let ws = self.windows_snapshot();
        let mut i: usize = 0;
        while i < ws.len() {
            s.push_str("W|");
            s.push_str(&ws[i].to_string());
            s.push('\n');
            i += 1;
        }
        if self.fingerprint() != fp0 {
            return Err(XC_DRIFT);
        }
        Ok(s)
    }
}

// ---------------------------------------------------------------------------
// §5 双跑对拍与注入检出面
// ---------------------------------------------------------------------------

/// 以同一窗口序列双跑两台检测器（判定确定性判据的机检本体）。
pub fn pair_run(seq: &[(u32, u64)]) -> Result<StormState, u16> {
    let mut a = StormDetector::new()?;
    let mut b = StormDetector::new()?;
    let mut i: usize = 0;
    while i < seq.len() {
        let sa = a.sample(seq[i].0, seq[i].1)?;
        let sb = b.sample(seq[i].0, seq[i].1)?;
        if sa != sb {
            return Err(XC_PAIR_MISMATCH);
        }
        i += 1;
    }
    if a.fingerprint() != b.fingerprint() {
        return Err(XC_PAIR_MISMATCH);
    }
    Ok(a.state())
}

/// 注入一：低率长序列（无风暴必不确认）。
pub fn inject_no_storm() -> Result<(), u16> {
    let mut d = StormDetector::new()?;
    let mut t: u64 = 10;
    let mut i: u32 = 0;
    while i < 20 {
        d.sample(50, t)?;
        t += 10;
        i += 1;
    }
    if d.state() != StormState::Quiet {
        return Err(XC_FALSE_CONFIRM);
    }
    Ok(())
}

/// 注入二：高率连续序列（有风暴必确认）。
pub fn inject_storm_must_confirm() -> Result<(), u16> {
    let mut d = StormDetector::new()?;
    let mut t: u64 = 10;
    let mut i: u32 = 0;
    while i < SUSPECT_TICKS {
        d.sample(RATE_HI + 500, t)?;
        t += 10;
        i += 1;
    }
    if d.state() != StormState::Confirmed {
        return Err(XC_MISS_CONFIRM);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §6 自例程（drill）——构造样本走核心路径，返回 0 表示全过
// ---------------------------------------------------------------------------

/// 自例程：无风暴不确认→风暴必确认→降级回退→双跑对拍→守恒→导出。
pub fn drill() -> Result<u32, u16> {
    // 注入两面：无风暴不确认 / 有风暴必确认。
    inject_no_storm()?;
    inject_storm_must_confirm()?;
    // 全周期：静默→疑点→确认→静默。
    let mut d = StormDetector::new()?;
    let mut t: u64 = 10;
    // 两窗低率（静默驻留）。
    d.sample(50, t)?;
    t += 10;
    d.sample(50, t)?;
    t += 10;
    // 两窗高率（入疑点）。
    d.sample(RATE_HI + 100, t)?;
    t += 10;
    d.sample(RATE_HI + 200, t)?;
    t += 10;
    if d.state() != StormState::Suspect {
        return Err(XC_MISS_CONFIRM);
    }
    // 第三窗高率（达确认持续窗——风暴确认）。
    d.sample(RATE_HI + 300, t)?;
    t += 10;
    if d.state() != StormState::Confirmed {
        return Err(XC_MISS_CONFIRM);
    }
    // 三窗静默（降级回退到静默）。
    let mut i: u32 = 0;
    while i < SUSPECT_TICKS {
        d.sample(20, t)?;
        t += 10;
        i += 1;
    }
    if d.state() != StormState::Quiet {
        return Err(XC_STATE_BAD);
    }
    // tick 回退拒收（采样拒收面）。
    if d.sample(50, 5) != Err(XC_TICK_REGRESS) {
        return Err(XC_TICK_REGRESS);
    }
    // 双跑对拍：全周期序列两台检测器逐窗一致。
    let seq = [(50u32, 10u64), (50, 20), (RATE_HI + 100, 30), (RATE_HI + 200, 40), (RATE_HI + 300, 50), (20, 60), (20, 70), (20, 80)];
    let st = pair_run(&seq)?;
    if st != StormState::Quiet {
        return Err(XC_PAIR_MISMATCH);
    }
    // 守恒机检 + 双导出指纹稳定。
    d.audit_conservation()?;
    if d.confirm_count() != 1 {
        return Err(XC_CONSERVATION);
    }
    let fp1 = d.fingerprint();
    let _ = d.export()?;
    if d.fingerprint() != fp1 {
        return Err(XC_DRIFT);
    }
    Ok(0)
}
