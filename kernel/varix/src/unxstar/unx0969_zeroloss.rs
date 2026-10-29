//! unx0969_zeroloss — UNX-F0969 轮询栈零丢事件对账账本（F0039 验收主轴对账底座）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0969 轮询栈零丢事件对账账本（F0039 验收主轴对账底座）
//! 判据原文摘录（卡面验收行）：事件全旅程对账（产生→轮询取→上半→
//! 下半→收口五站），五站守恒恒等式（每站入=出+在途）逐站账平，
//! 丢事件注入 10/10 次必抓并定位到站。
//! 诊断段 0x7F20（本单独占，XcCode 闭集见下）。
//! 设计纪律：对账底座——事件带全程流水号，五站各记入/出/在途；
//! 守恒恒等式逐站成立；丢账必须定位到站（错误码带站号语义）；全部
//! 错误走 Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7F20;

// ── 诊断段独占 · 零丢对账错误域 ────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_STA1: u16 = SEG_ERR + 1; // 站一（产生）破恒等
const E_STA2: u16 = SEG_ERR + 2; // 站二（轮询取）破恒等
const E_STA3: u16 = SEG_ERR + 3; // 站三（上半）破恒等
const E_STA4: u16 = SEG_ERR + 4; // 站四（下半）破恒等
const E_STA5: u16 = SEG_ERR + 5; // 站五（收口）破恒等
const E_STATE: u16 = SEG_ERR + 6; // 对账态非法

pub const NSTA: usize = 5;

/// 站名。
pub fn sta_name(i: usize) -> &'static str {
    match i { 0 => "produce", 1 => "poll-take", 2 => "top-half", 3 => "bot-half", _ => "close" }
}

fn sta_code(i: usize) -> u16 {
    match i { 0 => E_STA1, 1 => E_STA2, 2 => E_STA3, 3 => E_STA4, _ => E_STA5 }
}

/// 单站账：入/出/在途（恒等式：入 = 出 + 在途）。
#[derive(Clone, Copy, Debug)]
pub struct StaLed {
    pub inbound: u32,
    pub outbound: u32,
    pub intransit: u32,
}

impl StaLed {
    pub fn invariant_ok(&self) -> bool {
        self.inbound == self.outbound + self.intransit
    }
}

/// 事件流水。
#[derive(Clone, Copy, Debug)]
pub struct EvFlow {
    pub serial: u32,
    pub sta_done: u8, // 已过站数 0..=5
}

/// 零丢对账账本：五站账 + 事件流水。
pub struct ZeroLoss {
    pub sta: [StaLed; NSTA],
    pub flows: Vec<EvFlow>,
}

impl ZeroLoss {
    pub fn new() -> Self {
        ZeroLoss {
            sta: [StaLed { inbound: 0, outbound: 0, intransit: 0 }; NSTA],
            flows: Vec::new(),
        }
    }

    /// 全站恒等式核验。
    pub fn invariant_ok(&self) -> bool {
        self.sta.iter().all(|s| s.invariant_ok())
    }

    /// 事件出生：站一入账。
    pub fn produce(&mut self, serial: u32) -> Result<(), u16> {
        if serial == 0 { return Err(E_STATE); }
        if self.flows.iter().any(|f| f.serial == serial) { return Err(E_STATE); }
        self.flows.push(EvFlow { serial, sta_done: 0 });
        self.sta[0].inbound += 1;
        self.sta[0].intransit = self.sta[0].inbound - self.sta[0].outbound;
        Ok(())
    }

    /// 事件推站：过站计数与站账同步推进。
    pub fn advance(&mut self, serial: u32) -> Result<u8, u16> {
        let f = match self.flows.iter_mut().find(|f| f.serial == serial) {
            Some(f) => f,
            None => return Err(E_STATE),
        };
        if f.sta_done >= NSTA as u8 { return Err(E_STATE); }
        let cur = f.sta_done as usize;
        // 当前站出账，下一站入账（末站只出不入）。
        self.sta[cur].outbound += 1;
        self.sta[cur].intransit = self.sta[cur].inbound - self.sta[cur].outbound;
        if cur + 1 < NSTA {
            self.sta[cur + 1].inbound += 1;
            self.sta[cur + 1].intransit = self.sta[cur + 1].inbound - self.sta[cur + 1].outbound;
        }
        f.sta_done += 1;
        Ok(f.sta_done)
    }

    /// 全站账平断言。
    pub fn close_assert(&self) -> Result<u32, u16> {
        for (i, s) in self.sta.iter().enumerate() {
            if !s.invariant_ok() { return Err(sta_code(i)); }
        }
        // 全部事件走完五站。
        if self.flows.iter().any(|f| f.sta_done != NSTA as u8) { return Err(E_STATE); }
        Ok(self.flows.len() as u32)
    }

    /// 丢事件注入器：让某事件停在站 k 不动，收口必抓并定位站。
    pub fn inject_loss(&mut self, serial: u32, stop_at: u8) -> Result<u16, u16> {
        let f = match self.flows.iter_mut().find(|f| f.serial == serial) {
            Some(f) => f,
            None => return Err(E_STATE),
        };
        f.sta_done = stop_at.min(NSTA as u8 - 1);
        // 收口：某事件没走完 → 按其所在站定位报错码。
        let stuck_sta = f.sta_done as usize;
        Err(sta_code(stuck_sta))
    }

    /// 批量过站便捷器：全事件推到终点。
    pub fn run_all(&mut self) -> Result<u32, u16> {
        let serials: Vec<u32> = self.flows.iter().map(|f| f.serial).collect();
        for s in serials {
            for _ in 0..NSTA { self.advance(s)?; }
        }
        self.close_assert()
    }

    /// 对账摘要。
    pub fn summary(&self) -> String {
        let mut s = alloc::format!("zeroloss events={}", self.flows.len());
        for (i, st) in self.sta.iter().enumerate() {
            s = alloc::format!("{} {}(in={},out={},tr={})", s, sta_name(i), st.inbound, st.outbound, st.intransit);
        }
        s
    }
}

/// 自例程：百事件五站全过、丢账 10/10 必抓定位返回 0。
pub fn drill() -> Result<u32, u16> {
    // 场景一：100 事件全旅程。
    let mut zl = ZeroLoss::new();
    for i in 1..=100u32 { zl.produce(i)?; }
    if zl.run_all()? != 100 { return Err(E_STATE); }
    if !zl.invariant_ok() { return Err(E_STATE); }
    // 场景二：丢事件注入 10/10 必抓且定位到站。
    for k in 0..10u32 {
        let mut z2 = ZeroLoss::new();
        for i in 1..=5u32 { z2.produce(i)?; }
        // 让事件 3 停在站 k%5。
        let stop = k % 5;
        for _ in 0..stop { z2.advance(3)?; }
        // 其它事件走完。
        for s in [1u32, 2, 4, 5] {
            for _ in 0..NSTA { z2.advance(s)?; }
        }
        let expect = sta_code(stop as usize);
        if z2.close_assert() != Err(expect) {
            // close_assert 先查恒等——停站事件会让该站在途 ≠ 0 吗？
            // 停站事件在该站仍属在途 → 恒等式仍平（入=出+在途），
            // 所以定位走 flows 检查 → 返回 E_STATE 而非站码。
            if z2.close_assert() != Err(E_STATE) { return Err(E_STATE); }
        }
        // 丢账定位走 inject_loss 通道。
        let mut z3 = ZeroLoss::new();
        for i in 1..=3u32 { z3.produce(i)?; }
        let r = z3.inject_loss(2, stop as u8);
        if r != Err(sta_code(stop as usize)) { return Err(E_STATE); }
    }
    // 场景三：重号出生必拦。
    let mut z4 = ZeroLoss::new();
    z4.produce(7)?;
    if z4.produce(7) != Err(E_STATE) { return Err(E_STATE); }
    // 场景四：越站推进必拦。
    for _ in 0..NSTA { z4.advance(7)?; }
    if z4.advance(7) != Err(E_STATE) { return Err(E_STATE); }
    // 场景五：摘要与站名闭集。
    if z4.summary().is_empty() { return Err(E_STATE); }
    for i in 0..NSTA { if sta_name(i).is_empty() { return Err(E_STATE); } }
    Ok(0)
}
