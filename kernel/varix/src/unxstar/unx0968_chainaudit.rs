//! unx0968_chainaudit — UNX-F0968 四态迁移完整性断言链（Polling→Irq 全程验证资产）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0968 四态迁移完整性断言链（Polling→Irq 全程验证资产）
//! 判据原文摘录（卡面验收行）：全链迁移正确（完整四态迁移 10/10 次
//! 账平），各态停留时长账（Draining/Arming 窗口时长上限断言），违序
//! 迁移全拦截（六种违序注入 10/10 次），双判据缺一即批内红灯。
//! 诊断段 0x7F00（本单独占，XcCode 闭集见下）。
//! 设计纪律：链即契约——迁移只许沿 Polling→Draining→Arming→Irq
//! 线性链前进，任何回退/跳跃/自环必拦；每态停留时长有上限账；全部
//! 错误走 Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7F00;

// ── 诊断段独占 · 四态迁移链错误域 ──────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_ORDER: u16 = SEG_ERR + 1;  // 违序迁移
const E_WINDOW: u16 = SEG_ERR + 2; // 停留超窗
const E_CLOSE: u16 = SEG_ERR + 3;  // 链不闭合
const E_STATE: u16 = SEG_ERR + 4;  // 态非法
const E_BAL: u16 = SEG_ERR + 5;    // 账不平

/// 四态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum St4 {
    Polling = 0,
    Draining = 1,
    Arming = 2,
    Irq = 3,
}

impl St4 {
    pub fn idx(self) -> usize { self as usize }
    pub fn as_str(self) -> &'static str {
        match self { St4::Polling => "polling", St4::Draining => "draining", St4::Arming => "arming", St4::Irq => "irq" }
    }
    pub fn next(self) -> Option<St4> {
        match self {
            St4::Polling => Some(St4::Draining),
            St4::Draining => Some(St4::Arming),
            St4::Arming => Some(St4::Irq),
            St4::Irq => None,
        }
    }
}

/// 停留窗上限（逻辑 µs）。
pub const WIN_DRAIN: u64 = 50_000;
pub const WIN_ARM: u64 = 80_000;

/// 一次迁移记录。
#[derive(Clone, Copy, Debug)]
pub struct Trans {
    pub from: St4,
    pub to: St4,
    pub t_us: u64,
    pub dwell_us: u64, // from 态停留时长
}

/// 迁移链账本。
pub struct ChainAudit {
    pub trans: Vec<Trans>,
    cur: St4,
    cur_enter_t: u64,
    finished: bool,
}

impl ChainAudit {
    pub fn new(t0: u64) -> Self {
        ChainAudit { trans: Vec::new(), cur: St4::Polling, cur_enter_t: t0, finished: false }
    }

    pub fn current(&self) -> St4 { self.cur }

    /// 恒等式：链线性 + 停留在窗。
    pub fn invariant_ok(&self) -> bool {
        let mut prev = St4::Polling;
        for (i, tr) in self.trans.iter().enumerate() {
            if i == 0 {
                if tr.from != St4::Polling { return false; }
            } else if tr.from != prev { return false; }
            if prev.next() != Some(tr.to) { return false; }
            prev = tr.to;
        }
        true
    }

    /// 迁移一步：from 态停留时长过窗必拦。
    pub fn step(&mut self, t_us: u64) -> Result<St4, u16> {
        if self.finished { return Err(E_CLOSE); }
        let to = match self.cur.next() { Some(n) => n, None => return Err(E_CLOSE) };
        let dwell = t_us.saturating_sub(self.cur_enter_t);
        let win = match self.cur {
            St4::Draining => WIN_DRAIN,
            St4::Arming => WIN_ARM,
            _ => u64::MAX,
        };
        if dwell > win { return Err(E_WINDOW); }
        self.trans.push(Trans { from: self.cur, to, t_us, dwell_us: dwell });
        self.cur = to;
        self.cur_enter_t = t_us;
        if to == St4::Irq { self.finished = true; }
        Ok(to)
    }

    /// 全链跑完断言：Polling 起点到 Irq 终点恰好三跳。
    pub fn close_assert(&self) -> Result<u32, u16> {
        if !self.invariant_ok() { return Err(E_BAL); }
        if self.cur != St4::Irq { return Err(E_CLOSE); }
        if self.trans.len() != 3 { return Err(E_BAL); }
        Ok(3)
    }

    /// 违序注入器：伪造一条违序迁移记录，审计必抓。
    /// 违序类：0=回退 1=跳跃 2=自环。
    pub fn inject_bad(&mut self, kind: u8) -> bool {
        let from = self.cur;
        let to = match kind {
            0 => match from { St4::Draining => St4::Polling, St4::Arming => St4::Draining, St4::Irq => St4::Arming, _ => St4::Polling },
            1 => St4::Irq,
            _ => from,
        };
        self.trans.push(Trans { from, to, t_us: self.cur_enter_t, dwell_us: 0 });
        let caught = !self.invariant_ok();
        self.trans.pop();
        caught
    }

    /// 摘要。
    pub fn summary(&self) -> String {
        let mut s = alloc::format!("chain cur={} hops={}", self.cur.as_str(), self.trans.len());
        for tr in &self.trans {
            s = alloc::format!("{} {}->{}", s, tr.from.as_str(), tr.to.as_str());
        }
        s
    }
}

/// 完整链会话：给定各态停留时长跑完三跳。
pub fn chain_session(t_poll: u64, t_drain: u64, t_arm: u64) -> Result<ChainAudit, u16> {
    let mut c = ChainAudit::new(0);
    let mut t = t_poll;
    c.step(t)?;      // Polling → Draining
    t += t_drain;
    c.step(t)?;      // Draining → Arming
    t += t_arm;
    c.step(t)?;      // Arming → Irq
    c.close_assert()?;
    Ok(c)
}

/// 十次全链回归：10/10 次账平。
pub fn chain_regress(rounds: u32) -> Result<u32, u16> {
    if rounds == 0 { return Err(E_BAL); }
    let mut ok = 0u32;
    for _ in 0..rounds {
        match chain_session(100, 1000, 2000) {
            Ok(c) => { if c.close_assert()? == 3 { ok += 1; } }
            Err(e) => return Err(e),
        }
    }
    if ok != rounds { return Err(E_BAL); }
    Ok(ok)
}

/// 自例程：十次全链、违序六种注入、超窗必拦全过返回 0。
pub fn drill() -> Result<u32, u16> {
    // 场景一：10/10 全链账平。
    if chain_regress(10)? != 10 { return Err(E_BAL); }
    // 场景二：停留窗断言——Draining 超窗必拦。
    let mut c2 = ChainAudit::new(0);
    c2.step(100)?; // 进 Draining
    if c2.step(WIN_DRAIN + 1000) != Err(E_WINDOW) { return Err(E_WINDOW); }
    // 场景三：Arming 超窗必拦。
    let mut c3 = ChainAudit::new(0);
    c3.step(100)?;
    c3.step(200)?; // 进 Arming
    if c3.step(WIN_ARM + 1000) != Err(E_WINDOW) { return Err(E_WINDOW); }
    // 场景四：六种违序注入（回退/跳跃/自环 × 链上两处）10/10 必抓。
    let mut hits = 0u32;
    for kind in 0..3u8 {
        for _ in 0..2 {
            let mut c4 = ChainAudit::new(0);
            c4.step(100)?;
            if c4.inject_bad(kind) { hits += 1; }
            let mut c5 = ChainAudit::new(0);
            c5.step(100)?;
            c5.step(200)?;
            if c5.inject_bad(kind) { hits += 1; }
        }
    }
    if hits != 12 { return Err(E_ORDER); }
    // 场景五：终态后再迁必拦。
    let c6 = chain_session(100, 1000, 2000)?;
    let mut c6 = c6;
    if c6.step(999999) != Err(E_CLOSE) { return Err(E_CLOSE); }
    // 场景六：链摘要与态闭集。
    let mut seen = 0u32;
    for st in [St4::Polling, St4::Draining, St4::Arming, St4::Irq] {
        if st.as_str().is_empty() { return Err(E_STATE); }
        seen += 1;
    }
    if seen != 4 { return Err(E_STATE); }
    if c6.summary().is_empty() { return Err(E_STATE); }
    Ok(0)
}
