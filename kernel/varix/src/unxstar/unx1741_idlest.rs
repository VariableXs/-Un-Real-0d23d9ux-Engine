//! unx1741_idlest —— UNX-F1741 空闲循环总纲（idle 三态状态机）。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B08）：UNX-F1741 · 空闲循环总纲（idle 三态状态机） · 目标 320 行 · 已深化
//! 判据原文：UNX-F1741-J1 idle 进入/驻留/退出三态状态机冻结 v1，状态转换账
//!           与实际状态一致断言全过，非法转换注入 10/10 次拒绝
//! 组件：idle 三态状态机（Awake→Idle→Resident→Waking→Awake 语义；判据冻结
//!      三态闭集：Idle 进入态 / Resident 驻留态 / Awake 退出态）+
//!      转换账（每笔转换：from/to/tick/原因四列 append-only）+
//!      转换闸（合法转换表冻结：Awake→Idle→Resident→Awake 主环 +
//!      Resident→Awake 直接唤醒；其余全非法）+
//!      转换账 vs 实际态一致核（账末态 == 实际态恒等断言）+
//!      非法转换注入核（10 处 10/10 拒绝）+
//!      驻留统计（驻留时长分布：进入/退出 tick 差账）。
//! 职责定位：CPU 空闲态的唯一权威状态机——三态冻结使功耗与唤醒语义
//!   可审计；转换账是功耗账本（每笔进出有因有据）；非法转换全拒
//!   使 idle 面不会成为竞态入口（F1801 schedule 消费本态机）。
//! 性能逐项分解：转换 O(1)；账断言 O(1)（末账比对）；注入核 O(10)。
//!   全整数确定性（逻辑 tick 注入，零墙钟）。
//! 诊断码段：0x5330（0x5331~0x5338 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const IL_VERSION: &str = "unx1741_idlest-v1";

/// 诊断段基址（0x5330 段 · 本模块独占）。
pub const IL_SEG_BASE: u16 = 0x5330;

/// 三态闭集（判据点名：进入/驻留/退出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IlState {
    /// 清醒（退出态：有活干）。
    Awake,
    /// 进入（idle 已进入，正在降载）。
    Idle,
    /// 驻留（深度驻留，等唤醒源）。
    Resident,
}

impl IlState {
    /// 态号。
    pub fn num(self) -> u8 {
        match self {
            IlState::Awake => 0,
            IlState::Idle => 1,
            IlState::Resident => 2,
        }
    }
    /// 中文名。
    pub fn text(self) -> &'static str {
        match self {
            IlState::Awake => "清醒",
            IlState::Idle => "进入",
            IlState::Resident => "驻留",
        }
    }
}

/// 转换原因闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IlReason {
    /// 无活可跑进 idle。
    NoRunnable,
    /// 队列来活唤醒。
    WorkArrived,
    /// 深度驻留（短期无活确认）。
    DeepPark,
    /// 唤醒源命中（中断/定时器）。
    WakeSource,
    /// 驻留超时自醒。
    ResidentTimeout,
}

impl IlReason {
    /// 号。
    pub fn num(self) -> u8 {
        match self {
            IlReason::NoRunnable => 0,
            IlReason::WorkArrived => 1,
            IlReason::DeepPark => 2,
            IlReason::WakeSource => 3,
            IlReason::ResidentTimeout => 4,
        }
    }
    /// 中文。
    pub fn text(self) -> &'static str {
        match self {
            IlReason::NoRunnable => "无可跑进闲",
            IlReason::WorkArrived => "来活唤醒",
            IlReason::DeepPark => "深驻",
            IlReason::WakeSource => "唤醒源",
            IlReason::ResidentTimeout => "驻留超时",
        }
    }
}

/// 合法转换表（冻结：判据点名三态主环）。
/// Awake→Idle（NoRunnable）/ Idle→Resident（DeepPark）/ Idle→Awake（WorkArrived）/
/// Resident→Awake（WakeSource 或 ResidentTimeout）。
pub fn il_transition_legal(from: IlState, to: IlState, reason: IlReason) -> bool {
    match (from, to, reason) {
        (IlState::Awake, IlState::Idle, IlReason::NoRunnable) => true,
        (IlState::Idle, IlState::Resident, IlReason::DeepPark) => true,
        (IlState::Idle, IlState::Awake, IlReason::WorkArrived) => true,
        (IlState::Resident, IlState::Awake, IlReason::WakeSource) => true,
        (IlState::Resident, IlState::Awake, IlReason::ResidentTimeout) => true,
        _ => false,
    }
}

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IlCode {
    /// 0x5331 非法转换（转换表外）。
    IllegalTransition = 0x5331,
    /// 0x5332 转换账与实际态不一致。
    LedgerStateMismatch = 0x5332,
    /// 0x5333 转换账满（append-only 显性）。
    LedgerFull = 0x5333,
    /// 0x5334 账序倒退（tick 不单调）。
    TickBackward = 0x5334,
    /// 0x5335 驻留负时长。
    NegativeResidency = 0x5335,
    /// 0x5336 状态机冻结后写（封账后转换拒）。
    Sealed = 0x5336,
    /// 0x5337 重复进入（已 Idle 再进 Idle）。
    Reenter = 0x5337,
    /// 0x5338 转换原因空挂（合法态迁但原因不符）。
    ReasonMismatch = 0x5338,
}

impl IlCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }
    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            IlCode::IllegalTransition => "非法转换（转换表外）",
            IlCode::LedgerStateMismatch => "转换账与实际态不一致",
            IlCode::LedgerFull => "转换账满（显性溢出）",
            IlCode::TickBackward => "tick 倒退（账序单调破）",
            IlCode::NegativeResidency => "驻留负时长（退出早于进入）",
            IlCode::Sealed => "状态机封账后写拒",
            IlCode::Reenter => "重复进入（已闲再闲）",
            IlCode::ReasonMismatch => "转换原因空挂（态迁合法但原因不符）",
        }
    }
    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            IlCode::IllegalTransition => "按冻结转换表走；新转换语义须升版",
            IlCode::LedgerStateMismatch => "逐笔对账找漏记；账是唯一事实源",
            IlCode::LedgerFull => "封存当前账段开新段；不覆盖",
            IlCode::TickBackward => "tick 单调是账序契约：查时钟源",
            IlCode::NegativeResidency => "进入/退出 tick 配对检查",
            IlCode::Sealed => "封账后只读统计",
            IlCode::Reenter => "已闲态不重复进：查调度器重复 pick 空",
            IlCode::ReasonMismatch => "原因与转换配对见冻结表",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IlDiag {
    /// 码。
    pub code: IlCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl IlDiag {
    /// 构造。
    pub fn new(code: IlCode, site: &str, detail: String) -> Self {
        IlDiag { code, site: site.to_string(), detail }
    }
    /// 全文。
    pub fn full(&self) -> String {
        format!("[0x{:04X}] {} —— {} —— 建议:{}", self.code.num(), self.code.text(), self.detail, self.code.suggestion())
    }
}

/// 转换账条目（四列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IlEntry {
    /// from 态。
    pub from: IlState,
    /// to 态。
    pub to: IlState,
    /// tick。
    pub tick: u64,
    /// 原因。
    pub reason: IlReason,
}

/// idle 状态机（单核）。
#[derive(Clone, Debug)]
pub struct IlMachine {
    /// 实际态。
    pub state: IlState,
    /// 转换账（append-only）。
    pub ledger: Vec<IlEntry>,
    /// 账容量。
    pub cap: usize,
    /// 进入 tick（当前 idle 段起点；Awake 时为 0）。
    pub enter_tick: u64,
    /// 驻留统计。
    pub residencies: Vec<u64>,
    /// 封账。
    pub sealed: bool,
    /// 非法捕获计数。
    pub caught: u64,
}

impl IlMachine {
    /// 新状态机（清醒起点）。
    pub fn new(cap: usize) -> Self {
        IlMachine {
            state: IlState::Awake,
            ledger: Vec::new(),
            cap,
            enter_tick: 0,
            residencies: Vec::new(),
            sealed: false,
            caught: 0,
        }
    }

    /// 转换（判据点名：三态状态机 + 转换账一致 + 非法拒绝）。
    pub fn transit(&mut self, to: IlState, reason: IlReason, tick: u64) -> Result<(), IlDiag> {
        if self.sealed {
            return Err(IlDiag::new(IlCode::Sealed, "transit:seal", "已封账".to_string()));
        }
        if self.ledger.len() >= self.cap {
            return Err(IlDiag::new(IlCode::LedgerFull, "transit:cap", format!("账 {}", self.cap)));
        }
        // tick 单调（严格递增）。
        if let Some(last) = self.ledger.last() {
            if tick <= last.tick {
                return Err(IlDiag::new(IlCode::TickBackward, "transit:tick", format!("{} ≤ {}", tick, last.tick)));
            }
        }
        // 非法转换闸（转换表 + 原因配对双闸）。
        let from = self.state;
        if !il_transition_legal(from, to, reason) {
            self.caught += 1;
            // 细分：态迁本身非法 vs 原因不符。
            let reason_ok = match (from, to) {
                (IlState::Awake, IlState::Idle) => reason == IlReason::NoRunnable,
                (IlState::Idle, IlState::Resident) => reason == IlReason::DeepPark,
                (IlState::Idle, IlState::Awake) => reason == IlReason::WorkArrived,
                (IlState::Resident, IlState::Awake) => {
                    reason == IlReason::WakeSource || reason == IlReason::ResidentTimeout
                }
                _ => false,
            };
            if reason_ok {
                return Err(IlDiag::new(
                    IlCode::IllegalTransition,
                    "transit:pair",
                    format!("{:?}→{:?} 表外", from.text(), to.text()),
                ));
            }
            return Err(IlDiag::new(
                IlCode::ReasonMismatch,
                "transit:reason",
                format!("{:?}→{:?} 原因 {}", from.text(), to.text(), reason.text()),
            ));
        }
        // 重复进入语义（Awake→Idle 重复不存在——from 已查；Idle→Idle 表外已拒）。
        // 驻留时长记账（退出时配对）。
        if to == IlState::Awake && (from == IlState::Idle || from == IlState::Resident) {
            if tick < self.enter_tick {
                return Err(IlDiag::new(IlCode::NegativeResidency, "transit:res", "负驻留".to_string()));
            }
            self.residencies.push(tick - self.enter_tick);
        }
        if to == IlState::Idle {
            self.enter_tick = tick;
        }
        self.ledger.push(IlEntry { from, to, tick, reason });
        self.state = to;
        Ok(())
    }

    /// 账一致核（判据点名：状态转换账与实际状态一致断言全过）。
    pub fn ledger_consistent(&self) -> bool {
        match self.ledger.last() {
            None => self.state == IlState::Awake,
            Some(e) => e.to == self.state,
        }
    }

    /// 封账（只读统计模式）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 驻留统计：总驻留 / 最长驻留 / 段数。
    pub fn residency_stats(&self) -> (u64, u64, usize) {
        let total = self.residencies.iter().fold(0u64, |a, b| a.wrapping_add(*b));
        let max = self.residencies.iter().copied().max().unwrap_or(0);
        (total, max, self.residencies.len())
    }

    /// 报表。
    pub fn report(&self) -> String {
        let (total, max, n) = self.residency_stats();
        format!(
            "idle 态机 态:{} 转换:{} 驻留段:{} 总驻:{} 最长:{} 账一致:{} 捕获:{}",
            self.state.text(),
            self.ledger.len(),
            n,
            total,
            max,
            self.ledger_consistent(),
            self.caught
        )
    }
}

// ---------------------------------------------------------------------------
// 演练与注入（判据面）
// ---------------------------------------------------------------------------

/// 全绿演练：进闲→深驻→唤醒→再进闲→来活唤醒（账一致 + 驻留配对）。
pub fn drill_cycle() -> (bool, String) {
    let mut m = IlMachine::new(256);
    let r = (|| -> Result<(), IlDiag> {
        m.transit(IlState::Idle, IlReason::NoRunnable, 10)?;
        m.transit(IlState::Resident, IlReason::DeepPark, 20)?;
        m.transit(IlState::Awake, IlReason::WakeSource, 100)?;
        m.transit(IlState::Idle, IlReason::NoRunnable, 110)?;
        m.transit(IlState::Awake, IlReason::WorkArrived, 150)
    })();
    match r {
        Ok(()) => {
            let (total, max, n) = m.residency_stats();
            (
                m.ledger_consistent() && n == 2 && total == 130 && max == 80,
                format!("{} 总驻:{} 最长:{}", m.report(), total, max),
            )
        }
        Err(d) => (false, d.full()),
    }
}

/// 非法转换注入核：10 处 10/10 拒绝。
pub fn inject_illegal() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    // 注 1：Awake→Resident 直跳。
    let mut m = IlMachine::new(64);
    if m.transit(IlState::Resident, IlReason::DeepPark, 1).is_err() {
        caught += 1;
        detail.push("注1 直跳驻留:抓".to_string());
    } else {
        detail.push("注1 直跳驻留:漏".to_string());
    }
    // 注 2：Idle→Idle 重进。
    let mut m = IlMachine::new(64);
    let _ = m.transit(IlState::Idle, IlReason::NoRunnable, 1);
    if m.transit(IlState::Idle, IlReason::NoRunnable, 2).is_err() {
        caught += 1;
        detail.push("注2 重进:抓".to_string());
    } else {
        detail.push("注2 重进:漏".to_string());
    }
    // 注 3：Awake→Idle 用错原因（WorkArrived）。
    let mut m = IlMachine::new(64);
    if m.transit(IlState::Idle, IlReason::WorkArrived, 1).is_err() {
        caught += 1;
        detail.push("注3 进闲错因:抓".to_string());
    } else {
        detail.push("注3 进闲错因:漏".to_string());
    }
    // 注 4：Idle→Awake 用 DeepPark 原因。
    let mut m = IlMachine::new(64);
    let _ = m.transit(IlState::Idle, IlReason::NoRunnable, 1);
    if m.transit(IlState::Awake, IlReason::DeepPark, 2).is_err() {
        caught += 1;
        detail.push("注4 唤醒错因:抓".to_string());
    } else {
        detail.push("注4 唤醒错因:漏".to_string());
    }
    // 注 5：Resident→Resident。
    let mut m = IlMachine::new(64);
    let _ = m.transit(IlState::Idle, IlReason::NoRunnable, 1);
    let _ = m.transit(IlState::Resident, IlReason::DeepPark, 2);
    if m.transit(IlState::Resident, IlReason::DeepPark, 3).is_err() {
        caught += 1;
        detail.push("注5 驻留自环:抓".to_string());
    } else {
        detail.push("注5 驻留自环:漏".to_string());
    }
    // 注 6：Awake→Awake。
    let mut m = IlMachine::new(64);
    if m.transit(IlState::Awake, IlReason::NoRunnable, 1).is_err() {
        caught += 1;
        detail.push("注6 清醒自环:抓".to_string());
    } else {
        detail.push("注6 清醒自环:漏".to_string());
    }
    // 注 7：tick 倒退。
    let mut m = IlMachine::new(64);
    let _ = m.transit(IlState::Idle, IlReason::NoRunnable, 10);
    if m.transit(IlState::Awake, IlReason::WorkArrived, 5).is_err() {
        caught += 1;
        detail.push("注7 tick倒退:抓".to_string());
    } else {
        detail.push("注7 tick倒退:漏".to_string());
    }
    // 注 8：账满。
    let mut m = IlMachine::new(2);
    let _ = m.transit(IlState::Idle, IlReason::NoRunnable, 1);
    let _ = m.transit(IlState::Awake, IlReason::WorkArrived, 2);
    let _ = m.transit(IlState::Idle, IlReason::NoRunnable, 3);
    if m.transit(IlState::Awake, IlReason::WorkArrived, 4).is_err() {
        caught += 1;
        detail.push("注8 账满:抓".to_string());
    } else {
        detail.push("注8 账满:漏".to_string());
    }
    // 注 9：封账后写。
    let mut m = IlMachine::new(64);
    m.seal();
    if m.transit(IlState::Idle, IlReason::NoRunnable, 1).is_err() {
        caught += 1;
        detail.push("注9 封账后写:抓".to_string());
    } else {
        detail.push("注9 封账后写:漏".to_string());
    }
    // 注 10：账一致破坏检出（旁路改态）。
    let mut m = IlMachine::new(64);
    let _ = m.transit(IlState::Idle, IlReason::NoRunnable, 1);
    m.state = IlState::Awake; // 旁路改态
    if !m.ledger_consistent() {
        caught += 1;
        detail.push("注10 账态不一致:抓".to_string());
    } else {
        detail.push("注10 账态不一致:漏".to_string());
    }
    (caught, 10, detail)
}
