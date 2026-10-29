//! unx0959_bootlink — UNX-F0959 分发面启动序列挂接（F0897 衔接与三底座初始化序）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0959 分发面启动序列挂接（F0897 衔接与三底座初始化序）
//! 判据原文摘录（卡面验收行）：分发面挂接点对齐 F0897 启动序列
//! （挂接步号逐格落账），三底座初始化序不可逆序（softirq→tasklet→
//! workqueue），逆序挂接 10/10 次必拦，挂接账回放零改动。
//! 诊断段 0x7DE0（本单独占，XcCode 闭集见下）。
//! 设计纪律：挂接不碰时序——只在 F0897 序列的指定步号落账面挂接；
//! 三底座初始化有硬序（先有软中断槽位，再挂 tasklet，最后起工作
//! 线程）；重复挂接必拦；全部错误走 Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7DE0;

// ── 诊断段独占 · 启动挂接错误域 ────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_STEP: u16 = SEG_ERR + 1;  // 挂接步号非法
const E_ORDER: u16 = SEG_ERR + 2; // 三底座逆序
const E_DUP: u16 = SEG_ERR + 3;   // 重复挂接
const E_STATE: u16 = SEG_ERR + 4; // 挂接态非法
const E_REPLAY: u16 = SEG_ERR + 5; // 回放翻案

/// F0897 启动序列关键步号（域内契约）。
pub const STEP_IRQ_INIT: u32 = 10;   // 中断面初始化完成
pub const STEP_SOFTIRQ: u32 = 11;    // softirq 槽位就位
pub const STEP_TASKLET: u32 = 12;    // tasklet 底座就位
pub const STEP_WORKQ: u32 = 13;      // workqueue 底座就位
pub const STEP_DISPATCH_ON: u32 = 14; // 分发面开闸

/// 挂接记录。
#[derive(Clone, Copy, Debug)]
pub struct HookRec {
    pub step: u32,
    pub t_us: u64,
}

/// 三底座初始化状态位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BState {
    Off = 0,
    Ready = 1,
}

/// 挂接账本。
pub struct BootHook {
    pub hooks: Vec<HookRec>,
    softirq: BState,
    tasklet: BState,
    workq: BState,
    pub dispatch_on: bool,
}

impl BootHook {
    pub fn new() -> Self {
        BootHook { hooks: Vec::new(), softirq: BState::Off, tasklet: BState::Off, workq: BState::Off, dispatch_on: false }
    }

    /// 恒等式：底座就位硬序——tasklet 就位须 softirq 已就位，
    /// workqueue 就位须 tasklet 已就位。
    pub fn invariant_ok(&self) -> bool {
        let s = self.softirq as u32;
        let t = self.tasklet as u32;
        let w = self.workq as u32;
        !((t == 1 && s == 0) || (w == 1 && t == 0))
    }

    /// 挂接 softirq 槽位（步号必须恰为 STEP_SOFTIRQ）。
    pub fn hook_softirq(&mut self, step: u32, t_us: u64) -> Result<(), u16> {
        if self.dispatch_on { return Err(E_STATE); }
        if step != STEP_SOFTIRQ { return Err(E_STEP); }
        if self.softirq == BState::Ready { return Err(E_DUP); }
        self.softirq = BState::Ready;
        self.hooks.push(HookRec { step, t_us });
        Ok(())
    }

    /// 挂接 tasklet（前置：softirq 已就位）。
    pub fn hook_tasklet(&mut self, step: u32, t_us: u64) -> Result<(), u16> {
        if self.dispatch_on { return Err(E_STATE); }
        if step != STEP_TASKLET { return Err(E_STEP); }
        if self.tasklet == BState::Ready { return Err(E_DUP); }
        if self.softirq != BState::Ready { return Err(E_ORDER); }
        self.tasklet = BState::Ready;
        self.hooks.push(HookRec { step, t_us });
        Ok(())
    }

    /// 挂接 workqueue（前置：tasklet 已就位）。
    pub fn hook_workq(&mut self, step: u32, t_us: u64) -> Result<(), u16> {
        if self.dispatch_on { return Err(E_STATE); }
        if step != STEP_WORKQ { return Err(E_STEP); }
        if self.workq == BState::Ready { return Err(E_DUP); }
        if self.tasklet != BState::Ready { return Err(E_ORDER); }
        self.workq = BState::Ready;
        self.hooks.push(HookRec { step, t_us });
        Ok(())
    }

    /// 开闸（前置：三底座全就位）。
    pub fn hook_dispatch_on(&mut self, step: u32, t_us: u64) -> Result<(), u16> {
        if self.dispatch_on { return Err(E_DUP); }
        if step != STEP_DISPATCH_ON { return Err(E_STEP); }
        if self.softirq != BState::Ready || self.tasklet != BState::Ready || self.workq != BState::Ready {
            return Err(E_ORDER);
        }
        self.dispatch_on = true;
        self.hooks.push(HookRec { step, t_us });
        Ok(())
    }

    /// 回放：挂接步号必须严格递增且无重复。
    pub fn replay(&self) -> Result<u32, u16> {
        for w in self.hooks.windows(2) {
            if w[1].step <= w[0].step { return Err(E_REPLAY); }
        }
        Ok(self.hooks.len() as u32)
    }

    /// 就位摘要。
    pub fn summary(&self) -> String {
        alloc::format!("boothook steps={} softirq={:?} tasklet={:?} workq={:?} on={}",
            self.hooks.len(), self.softirq, self.tasklet, self.workq, self.dispatch_on as u8)
    }
}

/// 逆序注入器：尝试以错误顺序挂接（workq 先于 softirq 等），必拦。
pub fn inject_reverse_order(h: &mut BootHook) -> Result<bool, u16> {
    // 逆序一：tasklet 先于 softirq。
    if h.hook_tasklet(STEP_TASKLET, 0) == Err(E_ORDER) {
        return Ok(true);
    }
    // 若误挂成功则环境已破——报告失败。
    Err(E_STATE)
}

/// 自例程：正序挂接全绿、逆序 10/10 必拦、重复挂接必拦返回 0。
pub fn drill() -> Result<u32, u16> {
    // 场景一：正序全链挂接。
    let mut h = BootHook::new();
    h.hook_softirq(STEP_SOFTIRQ, 100)?;
    h.hook_tasklet(STEP_TASKLET, 110)?;
    h.hook_workq(STEP_WORKQ, 120)?;
    h.hook_dispatch_on(STEP_DISPATCH_ON, 130)?;
    if h.hooks.len() != 4 { return Err(E_STATE); }
    if h.replay()? != 4 { return Err(E_REPLAY); }
    if !h.dispatch_on { return Err(E_STATE); }
    if h.summary().is_empty() { return Err(E_STATE); }
    // 场景二：逆序注入 10/10 必拦（每次用新账本）。
    for _ in 0..10 {
        let mut h2 = BootHook::new();
        if inject_reverse_order(&mut h2)? != true { return Err(E_ORDER); }
    }
    // 场景三：跳步挂接必拦（softirq 未就位直接 workq）。
    let mut h3 = BootHook::new();
    if h3.hook_workq(STEP_WORKQ, 0) != Err(E_ORDER) { return Err(E_ORDER); }
    // 场景四：步号不符必拦。
    let mut h4 = BootHook::new();
    if h4.hook_softirq(STEP_IRQ_INIT, 0) != Err(E_STEP) { return Err(E_STEP); }
    if h4.hook_softirq(STEP_WORKQ, 0) != Err(E_STEP) { return Err(E_STEP); }
    // 场景五：重复挂接必拦。
    let mut h5 = BootHook::new();
    h5.hook_softirq(STEP_SOFTIRQ, 0)?;
    if h5.hook_softirq(STEP_SOFTIRQ, 1) != Err(E_DUP) { return Err(E_DUP); }
    h5.hook_tasklet(STEP_TASKLET, 1)?;
    if h5.hook_tasklet(STEP_TASKLET, 2) != Err(E_DUP) { return Err(E_DUP); }
    // 场景六：三底座未齐开闸必拦。
    let mut h6 = BootHook::new();
    h6.hook_softirq(STEP_SOFTIRQ, 0)?;
    if h6.hook_dispatch_on(STEP_DISPATCH_ON, 1) != Err(E_ORDER) { return Err(E_ORDER); }
    // 场景七：开闸后回写必拦。
    if h.hook_softirq(STEP_SOFTIRQ, 200) != Err(E_STATE) { return Err(E_STATE); }
    Ok(0)
}
