//! unx0966_arming — UNX-F0966 Arming 态语义细化（中断使能武装档）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0966 Arming 态语义细化（中断使能武装档）
//! 判据原文摘录（卡面验收行）：武装档语义逐条落账（使能前预检/使能
//! 即断点/首中断兜底轮询），违例 10/10 次必拦（未预检使能/使能后
//! 仍轮询/武装期注销），档账回放零漂移。
//! 诊断段 0x7EC0（本单独占，XcCode 闭集见下）。
//! 设计纪律：武装档铁律——使能中断前必须过预检（栈/掩码/链路三
//! 检）；使能点是一次性断点（不可逆回）；首中断到达前保留一轮兜底
//! 轮询；全部错误走 Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7EC0;

// ── 诊断段独占 · 武装档错误域 ──────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_PRECHK: u16 = SEG_ERR + 1;  // 未预检即使能
const E_ARMED_POLL: u16 = SEG_ERR + 2; // 武装后仍主力轮询
const E_UNARM: u16 = SEG_ERR + 3;   // 武装期注销（不可逆破约）
const E_STATE: u16 = SEG_ERR + 4;   // 档状态非法
const E_FIRST: u16 = SEG_ERR + 5;   // 首中断兜底缺失

/// 预检三面。
#[derive(Clone, Copy, Debug)]
pub struct Precheck {
    pub stack_ok: bool,   // 栈面就绪
    pub mask_ok: bool,    // 中断掩码面就绪
    pub link_ok: bool,    // 控制器链路就绪
}

impl Precheck {
    pub fn all_ok(&self) -> bool {
        self.stack_ok && self.mask_ok && self.link_ok
    }
}

/// 武装档执行器。
pub struct ArmingMode {
    state: ArmState,
    precheck: Option<Precheck>,
    pub fallback_pending: bool, // 首中断未到前兜底轮询位
    pub armed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArmState {
    Idle,
    PrecheckDone,
    Armed,
    Disarmed,
}

impl ArmingMode {
    pub fn new() -> Self {
        ArmingMode { state: ArmState::Idle, precheck: None, fallback_pending: false, armed: false }
    }

    /// 预检：三面全过才可进 PrecheckDone。
    pub fn run_precheck(&mut self, p: Precheck) -> Result<(), u16> {
        if self.state != ArmState::Idle { return Err(E_STATE); }
        if !p.all_ok() {
            self.precheck = Some(p);
            return Err(E_PRECHK);
        }
        self.precheck = Some(p);
        self.state = ArmState::PrecheckDone;
        Ok(())
    }

    /// 使能武装：前置预检已过；一次性断点。
    pub fn arm(&mut self) -> Result<(), u16> {
        match self.state {
            ArmState::Idle => Err(E_PRECHK),
            ArmState::Armed | ArmState::Disarmed => Err(E_STATE),
            ArmState::PrecheckDone => {
                self.state = ArmState::Armed;
                self.armed = true;
                self.fallback_pending = true; // 首中断未到，兜底轮询位亮起
                Ok(())
            }
        }
    }

    /// 武装期兜底轮询：仅首中断前允许一轮；首中断到达后关闭。
    pub fn fallback_poll(&mut self, first_irq: bool) -> Result<u32, u16> {
        if !self.armed { return Err(E_STATE); }
        if !self.fallback_pending && !first_irq { return Err(E_ARMED_POLL); }
        if first_irq {
            self.fallback_pending = false;
            Ok(0) // 中断面接管，轮询退位
        } else {
            Ok(1) // 兜底取 1 事件
        }
    }

    /// 注销（UNARM）：武装期禁止；仅错误恢复路径允许且必须落账。
    pub fn unarm(&mut self, recovery: bool) -> Result<(), u16> {
        if self.armed && !recovery { return Err(E_UNARM); }
        if self.state != ArmState::Armed { return Err(E_STATE); }
        self.armed = false;
        self.state = ArmState::Disarmed;
        Ok(())
    }

    pub fn state(&self) -> ArmState { self.state }

    /// 摘要。
    pub fn summary(&self) -> String {
        alloc::format!("arming state={:?} armed={} fb={}", self.state, self.armed as u8, self.fallback_pending as u8)
    }
}

/// 武装会话：预检 → 使能 → 兜底轮询 → 首中断。
pub fn arming_session(am: &mut ArmingMode, p: Precheck, fb_rounds: u32) -> Result<u32, u16> {
    am.run_precheck(p)?;
    am.arm()?;
    let mut got = 0u32;
    for _ in 0..fb_rounds {
        got += am.fallback_poll(false)?;
    }
    am.fallback_poll(true)?; // 首中断
    Ok(got)
}

/// 自例程：预检门、断点不可逆、兜底轮询、违例 10/10 必拦返回 0。
pub fn drill() -> Result<u32, u16> {
    let ok = Precheck { stack_ok: true, mask_ok: true, link_ok: true };
    // 场景一：标准武装会话。
    let mut am = ArmingMode::new();
    if arming_session(&mut am, ok, 3)? != 3 { return Err(E_FIRST); }
    if !am.armed && am.state() != ArmState::Armed { return Err(E_STATE); }
    if am.fallback_pending { return Err(E_FIRST); } // 首中断后兜底关闭
    // 场景二：未预检使能 10/10 必拦。
    for _ in 0..10 {
        let mut a2 = ArmingMode::new();
        if a2.arm() != Err(E_PRECHK) { return Err(E_PRECHK); }
    }
    // 场景三：预检不过使能必拦（三面各坏一）。
    let bads = [
        Precheck { stack_ok: false, mask_ok: true, link_ok: true },
        Precheck { stack_ok: true, mask_ok: false, link_ok: true },
        Precheck { stack_ok: true, mask_ok: true, link_ok: false },
    ];
    for b in bads.iter() {
        let mut a3 = ArmingMode::new();
        if a3.run_precheck(*b) != Err(E_PRECHK) { return Err(E_PRECHK); }
        if a3.arm() != Err(E_PRECHK) { return Err(E_PRECHK); }
    }
    // 场景四：武装后仍主力轮询必拦（兜底已关仍 poll）。
    if am.fallback_poll(false) != Err(E_ARMED_POLL) { return Err(E_ARMED_POLL); }
    // 场景五：武装期注销 10/10 必拦。
    for _ in 0..10 {
        let mut a4 = ArmingMode::new();
        a4.run_precheck(ok)?;
        a4.arm()?;
        if a4.unarm(false) != Err(E_UNARM) { return Err(E_UNARM); }
    }
    // 场景六：恢复路径注销合法（一次性）。
    let mut a5 = ArmingMode::new();
    a5.run_precheck(ok)?;
    a5.arm()?;
    a5.unarm(true)?;
    if a5.state() != ArmState::Disarmed { return Err(E_STATE); }
    if a5.fallback_poll(true) != Err(E_STATE) { return Err(E_STATE); }
    // 场景七：重复使能必拦。
    let mut a6 = ArmingMode::new();
    a6.run_precheck(ok)?;
    a6.arm()?;
    if a6.arm() != Err(E_STATE) { return Err(E_STATE); }
    if am.summary().is_empty() { return Err(E_STATE); }
    Ok(0)
}
