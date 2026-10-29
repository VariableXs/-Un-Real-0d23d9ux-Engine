//! unx0970_duobs — UNX-F0970 双模并行观测窗口（两模数据面对照观测）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0970 双模并行观测窗口（两模数据面对照观测）
//! 判据原文摘录（卡面验收行）：双模（纯轮询模/全中断模）并行观测
//! 窗口同源同事件对照落账，两模账面差异逐项可解释（吞吐差/时延差/
//! 丢失差三面），不可解释差异 10/10 次必拦，观测窗关闭时双账闭合。
//! 诊断段 0x7F40（本单独占，XcCode 闭集见下）。
//! 设计纪律：对照即双账——同一事件流灌入两模，各自记账；对照三面
//! （吞吐/时延/丢失）差异必须落在解释域内；窗口生命周期三态
//! （开窗/观测/闭窗）；全部错误走 Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7F40;

// ── 诊断段独占 · 双模观测错误域 ────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_DIFF: u16 = SEG_ERR + 1;  // 不可解释差异
const E_WIN: u16 = SEG_ERR + 2;   // 窗口态非法（未开窗观测等）
const E_LOPSIDE: u16 = SEG_ERR + 3; // 单模漏灌
const E_STATE: u16 = SEG_ERR + 4; // 账面状态非法
const E_CLOSE: u16 = SEG_ERR + 5; // 闭窗不闭合

/// 观测窗口三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinState {
    Open,
    Observing,
    Closed,
}

/// 单模账。
#[derive(Clone, Copy, Debug)]
pub struct ModeLed {
    pub events: u32,     // 吞吐面：事件数
    pub lat_sum_us: u64, // 时延面：总时延
    pub lost: u32,       // 丢失面：丢账数
}

impl ModeLed {
    fn new() -> Self { ModeLed { events: 0, lat_sum_us: 0, lost: 0 } }
    fn avg_lat(&self) -> u64 {
        if self.events == 0 { 0 } else { self.lat_sum_us / self.events as u64 }
    }
}

/// 双模对照差。
#[derive(Clone, Copy, Debug)]
pub struct Diff3 {
    pub thru: u32,  // 吞吐差（事件数差）
    pub lat: u64,   // 平均时延差（µs）
    pub lost: u32,  // 丢失差
}

impl Diff3 {
    /// 差异可解释域：吞吐差 ≤ 5%、时延差 ≤ 20%、丢失差 = 0。
    pub fn explainable(&self, base: u32, base_lat: u64) -> bool {
        let thru_ok = self.thru * 100 <= base * 5;
        let lat_ok = if base_lat == 0 { self.lat == 0 } else { self.lat * 100 <= base_lat * 20 };
        let lost_ok = self.lost == 0;
        thru_ok && lat_ok && lost_ok
    }
}

/// 双模观测窗口。
pub struct DuObsWin {
    state: WinState,
    pub polling: ModeLed,
    pub irq: ModeLed,
    pub diffs: Vec<Diff3>,
}

impl DuObsWin {
    pub fn new() -> Self {
        DuObsWin { state: WinState::Open, polling: ModeLed::new(), irq: ModeLed::new(), diffs: Vec::new() }
    }

    pub fn state(&self) -> WinState { self.state }

    /// 进观测态：开窗后许观测，闭窗后必拦。
    pub fn begin(&mut self) -> Result<(), u16> {
        if self.state != WinState::Open { return Err(E_WIN); }
        self.state = WinState::Observing;
        Ok(())
    }

    /// 灌事件：两模必须同步灌（单模漏灌必拦）。
    pub fn feed(&mut self, n: u32, poll_lat: u64, irq_lat: u64) -> Result<(), u16> {
        if self.state != WinState::Observing { return Err(E_WIN); }
        if n == 0 { return Err(E_LOPSIDE); }
        self.polling.events += n;
        self.polling.lat_sum_us += poll_lat * n as u64;
        self.irq.events += n;
        self.irq.lat_sum_us += irq_lat * n as u64;
        Ok(())
    }

    /// 记丢失：指定模记账。
    pub fn feed_lost(&mut self, polling_side: bool) -> Result<(), u16> {
        if self.state != WinState::Observing { return Err(E_WIN); }
        if polling_side { self.polling.lost += 1; } else { self.irq.lost += 1; }
        Ok(())
    }

    /// 对照落账：差异出域必拦。
    pub fn compare(&mut self) -> Result<Diff3, u16> {
        if self.state != WinState::Observing { return Err(E_WIN); }
        let base = self.irq.events.max(self.polling.events);
        let base_lat = self.irq.avg_lat().max(self.polling.avg_lat());
        let d = Diff3 {
            thru: self.irq.events.abs_diff(self.polling.events),
            lat: self.irq.avg_lat().abs_diff(self.polling.avg_lat()),
            lost: self.irq.lost.abs_diff(self.polling.lost),
        };
        if !d.explainable(base, base_lat) { return Err(E_DIFF); }
        self.diffs.push(d);
        Ok(d)
    }

    /// 闭窗：双账闭合（吞吐同源、差异全落账）才许关。
    pub fn close(&mut self) -> Result<u32, u16> {
        if self.state != WinState::Observing { return Err(E_CLOSE); }
        if self.polling.events != self.irq.events { return Err(E_LOPSIDE); }
        if self.diffs.is_empty() { return Err(E_DIFF); }
        self.state = WinState::Closed;
        Ok(self.diffs.len() as u32)
    }

    /// 对照摘要。
    pub fn summary(&self) -> String {
        alloc::format!("duobs state={:?} poll(ev={},lat={},lost={}) irq(ev={},lat={},lost={})",
            self.state, self.polling.events, self.polling.avg_lat(), self.polling.lost,
            self.irq.events, self.irq.avg_lat(), self.irq.lost)
    }
}

/// 自例程：同源双灌、差异解释域、闭窗闭合、违例 10/10 必拦返回 0。
pub fn drill() -> Result<u32, u16> {
    // 场景一：标准观测窗——10 轮对照。
    let mut w = DuObsWin::new();
    w.begin()?;
    for i in 0..10u32 {
        w.feed(100, 20 + i as u64, 18 + i as u64)?; // 时延差恒 2µs，在 20% 域内
        w.compare()?;
    }
    if w.close()? != 10 { return Err(E_DIFF); }
    if w.state() != WinState::Closed { return Err(E_STATE); }
    // 场景二：不可解释差异（时延差 >20%）必拦。
    for _ in 0..10 {
        let mut w2 = DuObsWin::new();
        w2.begin()?;
        w2.feed(100, 100, 10)?; // 轮询时延是中断 10 倍
        if w2.compare() != Err(E_DIFF) { return Err(E_DIFF); }
    }
    // 场景三：吞吐差 >5% 必拦。
    let mut w3 = DuObsWin::new();
    w3.begin()?;
    w3.feed(100, 40, 20)?;
    w3.polling.events += 10; // 单模多灌
    if w3.compare() != Err(E_DIFF) { return Err(E_DIFF); }
    // 场景四：丢失差 ≠ 0 必拦。
    let mut w4 = DuObsWin::new();
    w4.begin()?;
    w4.feed(100, 40, 20)?;
    w4.feed_lost(true)?;
    if w4.compare() != Err(E_DIFF) { return Err(E_DIFF); }
    // 场景五：未观测态灌账/对照/闭窗必拦。
    let mut w5 = DuObsWin::new();
    if w5.feed(1, 1, 1) != Err(E_WIN) { return Err(E_WIN); }
    if w5.compare() != Err(E_WIN) { return Err(E_WIN); }
    if w5.close() != Err(E_CLOSE) { return Err(E_CLOSE); }
    // 场景六：闭窗后再开必拦。
    if w.begin() != Err(E_WIN) { return Err(E_WIN); }
    // 场景七：零事件灌账必拦（单模漏灌语义）。
    let mut w6 = DuObsWin::new();
    w6.begin()?;
    if w6.feed(0, 0, 0) != Err(E_LOPSIDE) { return Err(E_LOPSIDE); }
    if w.summary().is_empty() { return Err(E_STATE); }
    Ok(0)
}
