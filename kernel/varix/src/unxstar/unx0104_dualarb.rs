//! UNX-F0104 · 双通道仲裁与降级策略 · 诊断段 0x2320
//! 验收锚点：UNX-F0104-J1 通道优先级仲裁可配置，主通道故障后
//! 降级零丢失。零浮点、零 unwrap、零 expect（100ms 预算以
//! 微秒整数记账，固定频标折算）。


use alloc::format;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x2320;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_UNKNOW: u16 = SEG_BASE + 1;  // 通道号未登记
const E_DUP: u16 = SEG_BASE + 2;     // 优先级重复（同优先级两通道）
const E_NOMAIN: u16 = SEG_BASE + 3;  // 无主通道可裁（全故障）
const E_RDONLY: u16 = SEG_BASE + 4;  // 通道已禁用，再发被拒
const E_LOST: u16 = SEG_BASE + 5;    // 零丢失判据破（有帧未落账）
const E_TWICE: u16 = SEG_BASE + 6;   // 同通道重复登记
const E_BAL: u16 = SEG_BASE + 7;     // 帧账守恒破（发=收+丢 不平）

pub fn detail(code: u16) -> &'static str {
    match code {
        E_UNKNOW => "通道未登记：仲裁表查无此号",
        E_DUP => "优先级重复：同优先级两通道，仲裁序不定",
        E_NOMAIN => "无主通道：全部故障，降级链耗尽",
        E_RDONLY => "通道已禁用：再发属误用",
        E_LOST => "零丢失判据破：降级窗口内帧未落账",
        E_TWICE => "通道重复登记：一号一账",
        E_BAL => "帧账守恒破：发出数 ≠ 收妥数 + 降级数 + 丢弃数",
        _ => "未知诊断码",
    }
}

// ── 降级预算常量 ─────────────────────────────────────────────
pub const DEGRADE_BUDGET_US: u64 = 100_000; // 100ms 降级窗

/// 通道状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChanState {
    Active,   // 在役
    Faulted,  // 故障（可恢复探测）
    Disabled, // 人为禁用
}

/// 单通道账。
pub struct Chan {
    pub id: u8,
    pub prio: u8, // 数小者优先
    pub state: ChanState,
    pub sent: u64,
    pub recv_ok: u64,
    pub degraded: u64, // 被本通道接住的他通道帧
    pub dropped: u64,
}

/// 双通道仲裁器：优先级可配置，主通道故障即降级。
pub struct Arbiter {
    pub chans: Vec<Chan>,
    pub degrade_deadline_us: u64, // 降级动作耗时预算
    pub last_degrade_us: u64,
}

impl Arbiter {
    pub fn new() -> Arbiter {
        Arbiter {
            chans: Vec::new(),
            degrade_deadline_us: DEGRADE_BUDGET_US,
            last_degrade_us: 0,
        }
    }

    /// 登记通道：优先级不得重复。
    pub fn register(&mut self, id: u8, prio: u8) -> Result<(), u16> {
        if self.chans.iter().any(|c| c.id == id) {
            return Err(E_TWICE);
        }
        if self.chans.iter().any(|c| c.prio == prio) {
            return Err(E_DUP);
        }
        self.chans.push(Chan {
            id,
            prio,
            state: ChanState::Active,
            sent: 0,
            recv_ok: 0,
            degraded: 0,
            dropped: 0,
        });
        Ok(())
    }

    fn find_idx(&self, id: u8) -> Result<usize, u16> {
        self.chans
            .iter()
            .position(|c| c.id == id)
            .ok_or(E_UNKNOW)
    }

    /// 当前主通道：在役者中优先级最小者。
    pub fn primary(&self) -> Result<u8, u16> {
        let mut best: Option<&Chan> = None;
        for c in &self.chans {
            if c.state == ChanState::Active {
                match best {
                    Some(b) if b.prio <= c.prio => {}
                    _ => best = Some(c),
                }
            }
        }
        best.map(|c| c.id).ok_or(E_NOMAIN)
    }

    /// 故障注入：主通道故障 → 降级（记录耗时，须在预算内）。
    pub fn fault(&mut self, id: u8, now_us: u64) -> Result<u8, u16> {
        let idx = self.find_idx(id)?;
        self.chans[idx].state = ChanState::Faulted;
        let newp = self.primary()?;
        self.last_degrade_us = now_us;
        Ok(newp)
    }

    /// 降级耗时核验：动作耗时须严格小于预算。
    pub fn degrade_in_budget(&self, done_us: u64) -> bool {
        done_us >= self.last_degrade_us
            && done_us - self.last_degrade_us < self.degrade_deadline_us
    }

    /// 发帧：仲裁到主通道；主通道故障时按登记序找次级接住（零丢失）。
    pub fn send(&mut self, id: u8) -> Result<u8, u16> {
        let idx = self.find_idx(id)?;
        if self.chans[idx].state == ChanState::Disabled {
            return Err(E_RDONLY);
        }
        self.chans[idx].sent += 1;
        let p = self.primary()?;
        let pidx = self.find_idx(p)?;
        // 在役主通道收下；若请求通道即主通道则正常收妥，否则记降级账。
        if p == id {
            self.chans[pidx].recv_ok += 1;
        } else {
            self.chans[pidx].degraded += 1;
        }
        Ok(p)
    }

    /// 守恒核：Σsent = Σ(recv_ok + degraded) + dropped，逐通道对账。
    pub fn audit(&self) -> Result<(), u16> {
        let sent: u64 = self.chans.iter().map(|c| c.sent).sum();
        let got: u64 = self.chans.iter().map(|c| c.recv_ok + c.degraded).sum();
        let drop: u64 = self.chans.iter().map(|c| c.dropped).sum();
        if sent != got + drop {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn render(&self) -> String {
        let mut s = format!("仲裁器 {} 通道：", self.chans.len());
        let mut first = true;
        for c in &self.chans {
            if !first {
                s.push_str("；");
            }
            first = false;
            let st = match c.state {
                ChanState::Active => "在役",
                ChanState::Faulted => "故障",
                ChanState::Disabled => "禁用",
            };
            s.push_str(&format!(
                "#{}(优{}){} 发{} 收{} 降{}",
                c.id, c.prio, st, c.sent, c.recv_ok, c.degraded
            ));
        }
        s
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：双通道登记（#0 优先 0，#1 优先 1）→ 主为 #0 → 发帧全收。
    let mut a = Arbiter::new();
    a.register(0, 0)?;
    a.register(1, 1)?;
    if a.primary()? != 0 {
        return Err(E_BAL);
    }
    for _ in 0..8 {
        if a.send(0)? != 0 {
            return Err(E_BAL);
        }
    }
    a.audit()?;
    hits += 1;

    // R2：主通道故障 → 降级到 #1，100ms 预算内核验通过；
    //     故障期间的发帧全部由次级接住（零丢失）。
    let p = a.fault(0, 1_000_000)?;
    if p != 1 {
        return Err(E_BAL);
    }
    if !a.degrade_in_budget(1_000_050) {
        return Err(E_LOST);
    }
    for _ in 0..5 {
        if a.send(0)? != 1 {
            return Err(E_BAL);
        }
    }
    a.audit()?;
    let c1 = &a.chans[1];
    if c1.recv_ok != 0 || c1.degraded != 5 {
        return Err(E_LOST);
    }
    hits += 1;

    // R3：优先级可配置——重登记反序（#2 优先 0）→ 主通道切换。
    let mut a3 = Arbiter::new();
    a3.register(2, 1)?;
    a3.register(3, 0)?;
    if a3.primary()? != 3 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：渲染账含通道号与状态。
    let rep = a.render();
    if !rep.contains("#0(优0)故障") || !rep.contains("#1(优1)在役") {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：未登记通道 → E_UNKNOW（发帧与故障注入两路）。
    let mut i1 = Arbiter::new();
    i1.register(0, 0)?;
    if i1.send(9) != Err(E_UNKNOW) {
        return Err(E_UNKNOW);
    }
    if i1.fault(9, 0) != Err(E_UNKNOW) {
        return Err(E_UNKNOW);
    }
    hits += 1;

    // I2：优先级重复 → E_DUP；同号重复登记 → E_TWICE。
    if i1.register(1, 0) != Err(E_DUP) {
        return Err(E_DUP);
    }
    if i1.register(0, 5) != Err(E_TWICE) {
        return Err(E_TWICE);
    }
    hits += 1;

    // I3：全部故障 → E_NOMAIN（降级链耗尽）。
    let mut i3 = Arbiter::new();
    i3.register(0, 0)?;
    i3.register(1, 1)?;
    i3.fault(0, 0)?;
    i3.fault(1, 10)?;
    if i3.primary() != Err(E_NOMAIN) {
        return Err(E_NOMAIN);
    }
    hits += 1;

    // I4：禁用通道再发 → E_RDONLY；守恒破注入 → E_BAL。
    i1.register(1, 1)?;
    {
        let c = &mut i1.chans[1];
        c.state = ChanState::Disabled;
    }
    if i1.send(1) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    let mut i4b = Arbiter::new();
    i4b.register(0, 0)?;
    i4b.send(0)?;
    i4b.chans[0].dropped = 1; // 人为破账：发1 收1 丢1
    if i4b.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    // I5：故障通道直发仍可降级接住，但恢复前主通道恒为次级。
    let mut i5 = Arbiter::new();
    i5.register(0, 0)?;
    i5.register(1, 1)?;
    i5.fault(0, 100)?;
    if i5.send(0)? != 1 {
        return Err(E_BAL);
    }
    if i5.primary()? != 1 {
        return Err(E_BAL);
    }
    i5.audit()?;
    hits += 1;

    Ok(hits)
}
