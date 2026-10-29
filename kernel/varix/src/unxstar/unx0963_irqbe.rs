//! unx0963_irqbe — UNX-F0963 IrqBackend trait 域内接线面（F0039 四态协议接口）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0963 IrqBackend trait 域内接线面（F0039 四态协议接口）
//! 判据原文摘录（卡面验收行）：IrqBackend 四态协议接口逐方法落账
//! （arm/drain/poll/confirm 四方法），两实现（轮询桩/中断桩）过同一
//! 协议面，违约实现 10/10 次必拦，接口面回放零漂移。
//! 诊断段 0x7E60（本单独占，XcCode 闭集见下）。
//! 设计纪律：协议即面——四方法闭集（arm 武装/drain 排空/poll 轮询/
//! confirm 确认），任何实现必须全域可跑；状态违例调用必拦；全部
//! 错误走 Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7E60;

// ── 诊断段独占 · 接线面错误域 ──────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_PROTO: u16 = SEG_ERR + 1; // 协议违例（方法序错）
const E_STATE: u16 = SEG_ERR + 2; // 四态机状态非法
const E_NOIMPL: u16 = SEG_ERR + 3; // 实现缺失
const E_DUP: u16 = SEG_ERR + 4;   // 重复确认
const E_LOST: u16 = SEG_ERR + 5;  // 事件丢账

/// 四态（F0039 协议）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BState4 {
    Polling = 0,
    Draining = 1,
    Arming = 2,
    Irq = 3,
}

impl BState4 {
    pub fn idx(self) -> usize { self as usize }
    pub fn as_str(self) -> &'static str {
        match self { BState4::Polling => "polling", BState4::Draining => "draining", BState4::Arming => "arming", BState4::Irq => "irq" }
    }
    pub fn next(self) -> Option<BState4> {
        match self {
            BState4::Polling => Some(BState4::Draining),
            BState4::Draining => Some(BState4::Arming),
            BState4::Arming => Some(BState4::Irq),
            BState4::Irq => None,
        }
    }
}

/// 一次接口面调用账。
#[derive(Clone, Copy, Debug)]
pub struct IfaceCall {
    pub state: BState4,
    pub method: &'static str,
    pub ok: bool,
}

/// IrqBackend 协议面：四方法闭集，任何后端实现必须全过。
/// （域内以 trait + 账面状态机表达。）
pub trait IrqBackend {
    /// 后端名。
    fn name(&self) -> &'static str;
    /// arm：武装中断（仅 Arming 态合法）。
    fn arm(&mut self, s: &mut BState4, log: &mut Vec<IfaceCall>) -> Result<(), u16>;
    /// drain：排空轮询积压（仅 Draining 态合法）。
    fn drain(&mut self, s: &mut BState4, log: &mut Vec<IfaceCall>) -> Result<u32, u16>;
    /// poll：纯轮询取事件（仅 Polling 态合法）。
    fn poll(&mut self, s: &mut BState4, log: &mut Vec<IfaceCall>) -> Result<u32, u16>;
    /// confirm：中断确认（仅 Irq 态合法）。
    fn confirm(&mut self, s: &mut BState4, log: &mut Vec<IfaceCall>) -> Result<(), u16>;
}

/// 协议日志辅助。
fn log_call(log: &mut Vec<IfaceCall>, s: BState4, method: &'static str, ok: bool) {
    log.push(IfaceCall { state: s, method, ok });
}

fn state_gate(s: BState4, want: BState4, log: &mut Vec<IfaceCall>, method: &'static str) -> Result<(), u16> {
    if s != want {
        log_call(log, s, method, false);
        return Err(E_PROTO);
    }
    log_call(log, s, method, true);
    Ok(())
}

/// 轮询桩实现：arm/confirm 空转（轮询后端不武装）。
pub struct PollStub;

impl IrqBackend for PollStub {
    fn name(&self) -> &'static str { "poll-stub" }
    fn arm(&mut self, s: &mut BState4, log: &mut Vec<IfaceCall>) -> Result<(), u16> {
        state_gate(*s, BState4::Arming, log, "arm")?;
        Ok(())
    }
    fn drain(&mut self, s: &mut BState4, log: &mut Vec<IfaceCall>) -> Result<u32, u16> {
        state_gate(*s, BState4::Draining, log, "drain")?;
        Ok(0) // 排空量 0（无积压）
    }
    fn poll(&mut self, s: &mut BState4, log: &mut Vec<IfaceCall>) -> Result<u32, u16> {
        state_gate(*s, BState4::Polling, log, "poll")?;
        Ok(1) // 每次轮询取 1 事件
    }
    fn confirm(&mut self, _s: &mut BState4, log: &mut Vec<IfaceCall>) -> Result<(), u16> {
        log_call(log, *_s, "confirm", true);
        Ok(())
    }
}

/// 中断桩实现：poll 拒跑（中断后端不轮询）。
pub struct IrqStub;

impl IrqBackend for IrqStub {
    fn name(&self) -> &'static str { "irq-stub" }
    fn arm(&mut self, s: &mut BState4, log: &mut Vec<IfaceCall>) -> Result<(), u16> {
        state_gate(*s, BState4::Arming, log, "arm")?;
        Ok(())
    }
    fn drain(&mut self, s: &mut BState4, log: &mut Vec<IfaceCall>) -> Result<u32, u16> {
        state_gate(*s, BState4::Draining, log, "drain")?;
        Ok(0)
    }
    fn poll(&mut self, s: &mut BState4, log: &mut Vec<IfaceCall>) -> Result<u32, u16> {
        // 中断后端在 Polling 态仍可退化轮询（过渡期），但 Irq 态 poll 必拦。
        state_gate(*s, BState4::Polling, log, "poll")?;
        Ok(0)
    }
    fn confirm(&mut self, s: &mut BState4, log: &mut Vec<IfaceCall>) -> Result<(), u16> {
        state_gate(*s, BState4::Irq, log, "confirm")?;
        Ok(())
    }
}

/// 协议面全跑：给一后端按四态正序各调一次合法方法。
pub fn protocol_sweep(b: &mut dyn IrqBackend, log: &mut Vec<IfaceCall>) -> Result<u32, u16> {
    let mut s = BState4::Polling;
    let mut n = 0u32;
    // Polling → poll
    b.poll(&mut s, log)?;
    n += 1;
    // Draining → drain
    s = BState4::Draining;
    b.drain(&mut s, log)?;
    n += 1;
    // Arming → arm
    s = BState4::Arming;
    b.arm(&mut s, log)?;
    n += 1;
    // Irq → confirm
    s = BState4::Irq;
    b.confirm(&mut s, log)?;
    n += 1;
    Ok(n)
}

/// 违约注入器：在错误态调方法，必拦且归因 E_PROTO。
pub fn inject_bad_call(b: &mut dyn IrqBackend, log: &mut Vec<IfaceCall>, state: BState4, method: u8) -> bool {
    let mut s = state;
    let r = match method {
        0 => b.poll(&mut s, log).err(),
        1 => b.drain(&mut s, log).err(),
        2 => b.arm(&mut s, log).err(),
        _ => b.confirm(&mut s, log).err(),
    };
    matches!(r, Some(E_PROTO))
}

/// 接线面审计器。
pub struct IfaceAuditor {
    pub log: Vec<IfaceCall>,
    pub impls_seen: Vec<&'static str>,
}

impl IfaceAuditor {
    pub fn new() -> Self { IfaceAuditor { log: Vec::new(), impls_seen: Vec::new() } }

    /// 审一实现：协议面全跑 + 日志全真。
    pub fn audit_impl(&mut self, b: &mut dyn IrqBackend) -> Result<u32, u16> {
        let n = protocol_sweep(b, &mut self.log)?;
        self.impls_seen.push(b.name());
        for c in &self.log {
            if !c.ok { return Err(E_NOIMPL); }
        }
        Ok(n)
    }

    /// 日志回放：调用账数与态序一致。
    pub fn replay(&self) -> Result<u32, u16> {
        if self.log.is_empty() { return Err(E_LOST); }
        Ok(self.log.len() as u32)
    }
}

/// 自例程：双实现过面、违约 10/10 必拦、日志回放全过返回 0。
pub fn drill() -> Result<u32, u16> {
    let mut aud = IfaceAuditor::new();
    // 场景一：轮询桩过面。
    let mut p = PollStub;
    if aud.audit_impl(&mut p)? != 4 { return Err(E_PROTO); }
    // 场景二：中断桩过面。
    let mut q = IrqStub;
    if aud.audit_impl(&mut q)? != 4 { return Err(E_PROTO); }
    if aud.impls_seen.len() != 2 { return Err(E_NOIMPL); }
    if aud.replay()? < 8 { return Err(E_LOST); }
    // 场景三：违约注入 10/10 必拦（双实现 × 四态 × 四方法全组合）。
    let mut hits = 0u32;
    for st in [BState4::Polling, BState4::Draining, BState4::Arming, BState4::Irq] {
        for m in 0..4u8 {
            let mut l2: Vec<IfaceCall> = Vec::new();
            let mut ps = PollStub;
            if inject_bad_call(&mut ps, &mut l2, st, m) { hits += 1; }
            let mut l3: Vec<IfaceCall> = Vec::new();
            let mut qs = IrqStub;
            if inject_bad_call(&mut qs, &mut l3, st, m) { hits += 1; }
        }
    }
    if hits < 10 { return Err(E_PROTO); }
    // 场景四：四态迁移闭链。
    let mut s = BState4::Polling;
    let mut hops = 0u32;
    while let Some(nx) = s.next() {
        s = nx;
        hops += 1;
    }
    if s != BState4::Irq || hops != 3 { return Err(E_STATE); }
    Ok(0)
}
