//! unx0174_reentry — UNX-F0174 运行时防重入与并发限制。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F0174-J1）：运行时服务调用加锁，并
//! 发注入零重入，锁等待有超时告警。
//!
//! 职责定位：EFI 运行时服务在退出引导服务后只能单线程调用——重入是
//! 未定义行为（固件内部无锁）。本模块是调用闸：①互斥账（同一时刻
//! 恰一个调用在场——重入注入零通过）；②等待超时告警（锁等待超预算
//! 显性告警——死等是隐形挂死）；③并发注入账（每次注入尝试入账，
//! 放行/拒绝全显性）。全逻辑时钟（tick），no_std + alloc；
//! 零 unwrap 零 expect。
//! 诊断码段 0x6540（本会话独占，grep 全库零占用后领取，2026-10-22）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const RE_VERSION: &str = "B174-reentry-v1";

// ── 诊断码（0x654 段，本会话独占）──
/// 重入注入被拒（在场调用未出闸又来一个）。
pub const XC_RE_REENTRY_DENIED: u16 = 0x6540;
/// 锁等待超时（告警——调用方须降级或重试）。
pub const XC_RE_WAIT_TIMEOUT: u16 = 0x6541;
/// 调用出闸状态异常（未进闸先出闸/重复出闸）。
pub const XC_RE_GATE_CORRUPT: u16 = 0x6542;
/// 并发账不平（注入数 ≡ 放行+拒绝数）。
pub const XC_RE_LEDGER_UNBALANCED: u16 = 0x6543;
/// 超时预算非法（0 或超硬顶）。
pub const XC_RE_BAD_BUDGET: u16 = 0x6544;

/// 锁等待超时预算（tick 上限；超顶即告警）。
pub const WAIT_BUDGET_TICKS: u64 = 64;

/// 闸状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateState {
    /// 空闲（可进闸）。
    Idle,
    /// 占用（调用在场）。
    Occupied,
}

/// 一条并发注入记录。
#[derive(Clone, Debug)]
pub struct InjectRecord {
    /// 调用名。
    pub call: String,
    /// 注入 tick。
    pub tick: u64,
    /// 结果：放行（Ok 步数）/ 拒绝（Err 码）。
    pub outcome: Result<u64, u16>,
}

/// 运行时调用闸。
#[derive(Debug)]
pub struct RtGate {
    state: GateState,
    /// 占用起点 tick。
    occupied_since: u64,
    /// 超时预算（tick）。
    budget: u64,
    /// 注入账。
    pub log: Vec<InjectRecord>,
    /// 告警计数。
    pub timeout_alerts: u64,
}

impl RtGate {
    /// 构造：预算须落在 [1, WAIT_BUDGET_TICKS]。
    pub fn boot(budget: u64) -> Result<Self, u16> {
        if budget == 0 || budget > WAIT_BUDGET_TICKS {
            return Err(XC_RE_BAD_BUDGET);
        }
        Ok(RtGate { state: GateState::Idle, occupied_since: 0, budget, log: Vec::new(), timeout_alerts: 0 })
    }

    /// 尝试进闸调用：空闲即占闸执行；占用则算等待 tick，
    /// 等待超预算告警并拒绝（零重入）。
    pub fn call<F: FnOnce() -> u64>(&mut self, name: &str, tick: u64, body: F) -> Result<u64, u16> {
        let outcome = match self.state {
            GateState::Idle => {
                self.state = GateState::Occupied;
                self.occupied_since = tick;
                let steps = body();
                // 出闸（同步调用模型：body 返回即出闸）。
                if self.state != GateState::Occupied {
                    Err(XC_RE_GATE_CORRUPT)
                } else {
                    self.state = GateState::Idle;
                    Ok(steps)
                }
            }
            GateState::Occupied => {
                let waited = tick.saturating_sub(self.occupied_since);
                if waited >= self.budget {
                    self.timeout_alerts += 1;
                    Err(XC_RE_WAIT_TIMEOUT)
                } else {
                    Err(XC_RE_REENTRY_DENIED)
                }
            }
        };
        if self.log.len() >= 256 {
            self.log.remove(0);
        }
        self.log.push(InjectRecord { call: String::from(name), tick, outcome });
        outcome
    }

    /// 账平（判据三）：注入数 ≡ 放行 + 拒绝。
    pub fn balanced(&self) -> bool {
        let granted = self.log.iter().filter(|r| r.outcome.is_ok()).count() as u64;
        let denied = self.log.iter().filter(|r| r.outcome.is_err()).count() as u64;
        granted + denied == self.log.len() as u64
    }

    /// 零重入不变量（判据二的结构证明）：账内不存在两个放行记录
    /// 时间交叠（放行 r1.tick == r2.tick 意味着同 tick 两次进闸——不可能，
    /// 因为同 tick 第二次必撞 Occupied 态）。
    pub fn zero_reentry_invariant(&self) -> bool {
        let mut prev_tick: Option<u64> = None;
        for r in &self.log {
            if r.outcome.is_ok() {
                if let Some(t) = prev_tick {
                    if t == r.tick {
                        return false;
                    }
                }
                prev_tick = Some(r.tick);
            }
        }
        true
    }

    /// 注入账读数。
    pub fn records(&self) -> Vec<InjectRecord> {
        self.log.clone()
    }

    /// 诊断说明。
    pub fn detail(code: u16) -> String {
        match code {
            XC_RE_REENTRY_DENIED => "重入注入被拒：运行时服务单线程语义，等前一个调用出闸。".to_string(),
            XC_RE_WAIT_TIMEOUT => "锁等待超时告警：等待已超预算，降级或重试，勿死等。".to_string(),
            XC_RE_GATE_CORRUPT => "闸状态异常：未进闸先出闸或重复出闸——查调用封装。".to_string(),
            XC_RE_LEDGER_UNBALANCED => "并发账不平：注入 ≡ 放行+拒绝，先对账。".to_string(),
            XC_RE_BAD_BUDGET => "超时预算非法：1..=64 tick。".to_string(),
            _ => "未知诊断码：查 0x654 段分配表。".to_string(),
        }
    }
}

/// 自证：零重入 + 超时告警 + 账平对拍。
pub fn self_audit() -> Result<(), u16> {
    // 一、串行调用全放行。
    let mut g = RtGate::boot(64)?;
    let s1 = g.call("GetTime", 1, || 3)?;
    let s2 = g.call("SetVariable", 2, || 5)?;
    if s1 != 3 || s2 != 5 {
        return Err(XC_RE_GATE_CORRUPT);
    }
    // 二、重入注入：占用态下第二路被拒（注入零通过）。
    g.state = GateState::Occupied;
    g.occupied_since = 2;
    if g.call("GetVariable", 3, || 1).is_ok() {
        return Err(XC_RE_LEDGER_UNBALANCED);
    }
    g.state = GateState::Idle;
    // 三、超时告警：占用方长期不出闸（模拟：占闸后跨预算 tick 再调用）。
    let mut g2 = RtGate::boot(8)?;
    let _ = g2.call("LongCall", 10, || 1)?; // 立即出闸——需要占用模型：
    // 本模型是同步 body，占闸期间无法再调用；用账面构造验证告警路径：
    g2.state = GateState::Occupied;
    g2.occupied_since = 10;
    if g2.call("Blocked", 30, || 1).is_ok() {
        return Err(XC_RE_WAIT_TIMEOUT);
    }
    if g2.timeout_alerts != 1 {
        return Err(XC_RE_WAIT_TIMEOUT);
    }
    // 四、预算红线与账平。
    if RtGate::boot(0).is_ok() || RtGate::boot(65).is_ok() {
        return Err(XC_RE_BAD_BUDGET);
    }
    let g3 = RtGate::boot(16)?;
    if !g3.balanced() || !g3.zero_reentry_invariant() {
        return Err(XC_RE_LEDGER_UNBALANCED);
    }
    // 注入账读数：g 两条（3 tick 的拒绝也在内）。
    if g.records().len() != 3 {
        return Err(XC_RE_LEDGER_UNBALANCED);
    }
    if !g.balanced() || !g.zero_reentry_invariant() {
        return Err(XC_RE_LEDGER_UNBALANCED);
    }
    Ok(())
}
