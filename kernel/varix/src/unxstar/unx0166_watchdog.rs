//! unx0166_watchdog — UNX-F0166 EFI Watchdog 处理。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F0166-J1）：ExitBootServices 时固件
//! watchdog 置 5 分钟语义保留并记录，超时事件路径有账。
//!
//! 职责定位：固件在 ExitBootServices 时启动 5 分钟看门狗——内核若 5 分
//! 钟内没接走超时路径（自装定时器/关狗），固件直接复位且无账可查。本模
//! 块三段账：①交接口账（EBS 时刻的 5 分钟预算登记——语义保留：值不归
//! 零不改，按固件语义记账）；②接管账（内核自装定时器或显式关狗的时点
//! 与方式——谁在什么逻辑时刻接走的）；③超时账（超时事件路径：从预算
//! 耗尽到复位/ rescued 的完整事件链——超时不再是"黑箱复位"）。全逻辑
//! 时钟（tick），无墙钟；no_std + alloc；零 unwrap 零 expect。
//! 诊断码段 0x2870（本会话独占，grep 全库零占用后领取，2026-10-22）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const WD_VERSION: &str = "B166-efiwatchdog-v1";

// ── 诊断码（0x287 段，本会话独占）──
/// 看门狗未交接就超时（固件复位路径触发——最坏事件）。
pub const XC_WD_TIMEOUT_RESET: u16 = 0x2870;
/// 重复交接（内核已接管后再次走固件语义——二义账）。
pub const XC_WD_DUP_HANDOVER: u16 = 0x2871;
/// 预算非法（5 分钟语义外的登记——口径破坏）。
pub const XC_WD_BAD_BUDGET: u16 = 0x2872;
/// 超时账不平（事件链断裂：耗尽但无终态）。
pub const XC_WD_LEDGER_UNBALANCED: u16 = 0x2873;
/// 交接发生在超时之后（时序倒置——账目不可能事件）。
pub const XC_WD_LATE_HANDOVER: u16 = 0x2874;

/// 固件语义：EBS 后看门狗 5 分钟（300 秒）。
pub const WATCHDOG_BUDGET_SEC: u64 = 300;
/// 逻辑 tick 粒度（1 tick = 1 秒）。
pub const TICK_SEC: u64 = 1;

/// 看门狗状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WdState {
    /// 固件持有（EBS 前的常态，不计时）。
    FirmwareHeld,
    /// 已交接给固件语义（EBS 后，5 分钟倒计时中）。
    ArmedByFirmware,
    /// 内核已接管（自装定时器/关狗——超时路径归内核）。
    TakenByKernel,
    /// 超时已发生（复位或被救）。
    Expired,
}

/// 一条超时事件链记录。
#[derive(Clone, Debug)]
pub struct WdEvent {
    /// 逻辑时刻（tick）。
    pub tick: u64,
    /// 事件名。
    pub what: String,
}

/// EFI 看门狗三段账。
#[derive(Debug)]
pub struct WatchdogLedger {
    state: WdState,
    /// EBS 时刻（armed 起点 tick）。
    armed_at: u64,
    /// 预算（恒 300——语义保留）。
    budget: u64,
    /// 事件链（append-only）。
    pub events: Vec<WdEvent>,
    /// 内核接管方式（Some=方式说明）。
    pub takeover: Option<String>,
}

impl WatchdogLedger {
    /// EBS 时刻登记：进入固件 5 分钟语义（记录但不改值）。
    pub fn exit_boot_services(&mut self, tick: u64) -> Result<(), u16> {
        if self.state != WdState::FirmwareHeld {
            return Err(XC_WD_DUP_HANDOVER);
        }
        if self.budget != WATCHDOG_BUDGET_SEC {
            return Err(XC_WD_BAD_BUDGET);
        }
        self.state = WdState::ArmedByFirmware;
        self.armed_at = tick;
        self.events.push(WdEvent { tick, what: String::from("EBS：固件看门狗 5 分钟语义保留登记") });
        Ok(())
    }

    /// tick 推进：检查超时（Armed 态才有超时概念）。
    /// 返回 Ok(true)=已超时（进入 Expired，事件链记录完整）。
    pub fn tick(&mut self, now: u64) -> Result<bool, u16> {
        if self.state != WdState::ArmedByFirmware {
            return Ok(false);
        }
        let elapsed = now.saturating_sub(self.armed_at);
        if elapsed >= self.budget {
            self.state = WdState::Expired;
            self.events.push(WdEvent { tick: now, what: String::from("超时：5 分钟预算耗尽，固件复位路径触发（有账）") });
            return Ok(true);
        }
        Ok(false)
    }

    /// 内核接管（须在超时前）：记录方式与时点。
    pub fn kernel_takeover(&mut self, tick: u64, how: &str) -> Result<(), u16> {
        if self.state == WdState::Expired {
            return Err(XC_WD_LATE_HANDOVER);
        }
        if self.state != WdState::ArmedByFirmware {
            return Err(XC_WD_DUP_HANDOVER);
        }
        if tick < self.armed_at {
            return Err(XC_WD_LATE_HANDOVER);
        }
        let elapsed = tick - self.armed_at;
        if elapsed >= self.budget {
            return Err(XC_WD_LATE_HANDOVER);
        }
        self.state = WdState::TakenByKernel;
        self.takeover = Some(String::from(how));
        self.events.push(WdEvent { tick, what: String::from("内核接管：") + how });
        Ok(())
    }

    /// 超时账平：Expired 态必有终态事件；非 Expired 态不得有超时事件。
    pub fn ledger_balanced(&self) -> bool {
        let has_timeout = self.events.iter().any(|e| e.what.contains("超时"));
        match self.state {
            WdState::Expired => has_timeout,
            _ => !has_timeout,
        }
    }

    /// 交接时点读数（审计）。
    pub fn snapshot(&self) -> (WdState, u64, u64) {
        (self.state, self.armed_at, self.budget)
    }

    /// 诊断说明。
    pub fn detail(code: u16) -> String {
        match code {
            XC_WD_TIMEOUT_RESET => "看门狗超时复位：查事件链确认 5 分钟内为何未接管。".to_string(),
            XC_WD_DUP_HANDOVER => "重复交接：接管动作只许一次，先查当前状态。".to_string(),
            XC_WD_BAD_BUDGET => "预算非法：固件语义恒 5 分钟（300 秒），不许私改口径。".to_string(),
            XC_WD_LEDGER_UNBALANCED => "超时账不平：Expired 必有终态事件，非 Expired 不得有超时事件。".to_string(),
            XC_WD_LATE_HANDOVER => "交接时序倒置：超时后或 EBS 前的接管都是不可能事件。".to_string(),
            _ => "未知诊断码：查 0x287 段分配表。".to_string(),
        }
    }
}

/// 自证：三段账的正反面对拍。
pub fn self_audit() -> Result<(), u16> {
    // 一、正常路径：EBS 登记 → 第 100 tick 接管 → 无超时。
    let mut w = WatchdogLedger { state: WdState::FirmwareHeld, armed_at: 0, budget: WATCHDOG_BUDGET_SEC, events: Vec::new(), takeover: None };
    w.exit_boot_services(10)?;
    w.tick(50)?;
    w.kernel_takeover(110, "内核自装 HPET 定时器，固件狗等价停用")?;
    if w.tick(200)? {
        return Err(XC_WD_TIMEOUT_RESET);
    }
    if !w.ledger_balanced() || w.takeover.is_none() {
        return Err(XC_WD_LEDGER_UNBALANCED);
    }
    let (st, at, bd) = w.snapshot();
    if st != WdState::TakenByKernel || at != 10 || bd != 300 {
        return Err(XC_WD_BAD_BUDGET);
    }

    // 二、超时路径：EBS 后不接管 → 第 310 tick 超时（有账）。
    let mut w2 = WatchdogLedger { state: WdState::FirmwareHeld, armed_at: 0, budget: WATCHDOG_BUDGET_SEC, events: Vec::new(), takeover: None };
    w2.exit_boot_services(0)?;
    let expired = w2.tick(310)?;
    if !expired || !w2.ledger_balanced() {
        return Err(XC_WD_LEDGER_UNBALANCED);
    }
    // 超时后接管 → 时序倒置拒。
    if w2.kernel_takeover(320, "迟到接管").is_ok() {
        return Err(XC_WD_LATE_HANDOVER);
    }

    // 三、纪律：重复 EBS 拒、EBS 前接管拒、预算篡改拒。
    let mut w3 = WatchdogLedger { state: WdState::FirmwareHeld, armed_at: 0, budget: WATCHDOG_BUDGET_SEC, events: Vec::new(), takeover: None };
    w3.exit_boot_services(5)?;
    if w3.exit_boot_services(6).is_ok() {
        return Err(XC_WD_DUP_HANDOVER);
    }
    let mut w4 = WatchdogLedger { state: WdState::FirmwareHeld, armed_at: 0, budget: WATCHDOG_BUDGET_SEC, events: Vec::new(), takeover: None };
    if w4.kernel_takeover(1, "EBS 前接管").is_ok() {
        return Err(XC_WD_LATE_HANDOVER);
    }
    let mut w5 = WatchdogLedger { state: WdState::FirmwareHeld, armed_at: 0, budget: 60, events: Vec::new(), takeover: None };
    if w5.exit_boot_services(0).is_ok() {
        return Err(XC_WD_BAD_BUDGET);
    }
    Ok(())
}
