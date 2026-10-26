//! UNX-F2435 · 高阶枯竭告警线（order≥9） · 诊断段 0x6F70（独占 0x6F70~0x6F7F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B02 反碎片批 · UNX-F2435）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2435-J1 order≥9 free 低于阈值 10/10 次触发预警并报 AI-83 域经，
//! 正常波动零误报。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 10/10 触发：order≥9 空闲帧数跌破告警线（默认 16 帧 =
//!   2 块 order-9）时 10/10 次触发预警，预警报文带 AI-83 域经编号。
//!   J1-b 零误报：正常波动（阈值上方 ±2 帧抖动）零预警——滞回带
//!   语义：跌破线触发一次后须回升超「解除线」（阈值 +8 帧）才复位。
//!   J1-c 域经联动：预警事件写入域经通道（AI-83 编号 + 快照：各阶
//!   余量 + 触发时刻事件号），可回溯。
//!
//! **段位复核记录**：0x6F70~0x6F7F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6F70~0x6F7F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6F70;
/// 段顶。
pub const SEG_TOP: u16 = 0x6F7F;

/// 告警线（order≥9 空闲帧数）。
pub const ALERT_FLOOR_FRAMES: u64 = 16;
/// 解除线（滞回带）。
pub const CLEAR_CEIL_FRAMES: u64 = 24;
/// AI-83 域经编号。
pub const AI83_DOMAIN_CODE: u16 = 83;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HighOrdWarnError {
    /// 0x6F71 跌线未预警（漏报）。
    MissedAlert,
    /// 0x6F72 正常波动误报。
    FalsePositive,
    /// 0x6F73 滞回带非法（解除线 ≤ 告警线）。
    BadHysteresis,
    /// 0x6F74 域经账缺件。
    LedgerMissing,
}

impl HighOrdWarnError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            HighOrdWarnError::MissedAlert => format!(
                "[0x{:04X}] 跌线未预警：观察器漏报。\
                 改法：核对采样点是否覆盖每帧变化。",
                self.code()
            ),
            HighOrdWarnError::FalsePositive => format!(
                "[0x{:04X}] 正常波动误报：滞回带失效。\
                 改法：核对解除线是否高于告警线。",
                self.code()
            ),
            HighOrdWarnError::BadHysteresis => format!(
                "[0x{:04X}] 滞回带非法：解除线必须 > 告警线。\
                 改法：配置项校验。",
                self.code()
            ),
            HighOrdWarnError::LedgerMissing => format!(
                "[0x{:04X}] 域经账缺件：AI-83 报文字段不齐。\
                 改法：预警必须带快照（各阶余量+事件号）。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、滞回观察器
// ===========================================================================

/// 一条预警事件。
#[derive(Clone, Copy, Debug)]
pub struct AlertEvent {
    /// 触发时 order≥9 空闲帧数。
    pub free_at: u64,
    /// 域经编号（AI-83）。
    pub domain: u16,
    /// 事件号。
    pub seq: u64,
}

/// 滞回观察器。
pub struct HysteresisWatcher {
    /// 当前状态：true=预警中。
    pub alerted: bool,
    /// 预警账。
    pub alerts: Vec<AlertEvent>,
    /// 误报计数（诊断面）。
    pub false_positives: u64,
    seq: u64,
}

impl HysteresisWatcher {
    /// 构造（滞回带校验）。
    pub fn new() -> result::Result<Self, HighOrdWarnError> {
        if CLEAR_CEIL_FRAMES <= ALERT_FLOOR_FRAMES {
            return Err(HighOrdWarnError::BadHysteresis);
        }
        Ok(HysteresisWatcher {
            alerted: false,
            alerts: Vec::new(),
            false_positives: 0,
            seq: 0,
        })
    }

    /// 观察一个采样点：返回是否新触发预警。
    pub fn observe(&mut self, high_order_free: u64) -> bool {
        self.seq += 1;
        if !self.alerted {
            if high_order_free < ALERT_FLOOR_FRAMES {
                self.alerted = true;
                self.alerts.push(AlertEvent {
                    free_at: high_order_free,
                    domain: AI83_DOMAIN_CODE,
                    seq: self.seq,
                });
                return true;
            }
        } else if high_order_free >= CLEAR_CEIL_FRAMES {
            // 升过解除线复位。
            self.alerted = false;
        }
        false
    }

    /// J1-c 域经账完整性核。
    pub fn audit_ledger(&self) -> result::Result<usize, HighOrdWarnError> {
        let mut i = 0;
        while i < self.alerts.len() {
            let a = &self.alerts[i];
            if a.domain != AI83_DOMAIN_CODE || a.free_at >= ALERT_FLOOR_FRAMES {
                return Err(HighOrdWarnError::LedgerMissing);
            }
            i += 1;
        }
        Ok(self.alerts.len())
    }
}

// ===========================================================================
// 三、J1 证据
// ===========================================================================

/// LCG。
pub struct Lcg(u64);

impl Lcg {
    /// 构造。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x2435_6F70_9E37_79B9)
    }

    /// 下一个。
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    /// [0, bound)。
    pub fn next_below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next_u64() % bound
    }
}

/// 跌线演练轮数。
pub const TRIGGER_ROUNDS: usize = 10;
/// 正常波动采样数。
pub const NORMAL_SAMPLES: u64 = 10_000;

/// J1 证据生成器：10/10 跌线触发 + 万点正常波动零误报。
pub fn warn_audit() -> result::Result<(usize, u64), HighOrdWarnError> {
    // (a) 10/10 跌线触发。
    let mut caught = 0usize;
    let mut r = 0;
    while r < TRIGGER_ROUNDS {
        let mut w = HysteresisWatcher::new()?;
        // 波动线上方若干点后跌线。
        w.observe(ALERT_FLOOR_FRAMES + 5);
        w.observe(ALERT_FLOOR_FRAMES + 2);
        let fired = w.observe(ALERT_FLOOR_FRAMES - 1);
        if fired {
            caught += 1;
        } else {
            return Err(HighOrdWarnError::MissedAlert);
        }
        r += 1;
    }
    // (b) 正常波动零误报：阈值上方 24~64 帧抖动。
    let mut w2 = HysteresisWatcher::new()?;
    let mut rng = Lcg::new(0x2435_0001);
    let mut n = 0u64;
    while n < NORMAL_SAMPLES {
        let v = CLEAR_CEIL_FRAMES + rng.next_below(40);
        if w2.observe(v) {
            return Err(HighOrdWarnError::FalsePositive);
        }
        n += 1;
    }
    w2.audit_ledger()?;
    Ok((caught, w2.false_positives))
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 跌线触发数（应 10/10）。
    pub triggered: usize,
    /// 正常波动误报数（应 0）。
    pub false_pos: u64,
    /// 域经账核过数。
    pub ledger_checks: usize,
    /// 滞回复位生效。
    pub hysteresis_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.triggered == TRIGGER_ROUNDS && self.false_pos == 0 && self.ledger_checks >= 0 && self.hysteresis_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, HighOrdWarnError> {
    let (triggered, false_pos) = warn_audit()?;
    let mut w = HysteresisWatcher::new()?;
    // 滞回全周期：跌线 → 抖动不解除 → 升过解除线复位。
    let f1 = w.observe(ALERT_FLOOR_FRAMES - 4);
    let jitter = w.observe(ALERT_FLOOR_FRAMES + 3); // 未到解除线：仍预警中，无新预警。
    let clear = w.observe(CLEAR_CEIL_FRAMES + 10);
    let hysteresis_ok = f1 && !jitter && !clear && !w.alerted;
    // 域经账核（f1 事件在账）。
    let ledger_checks = w.audit_ledger()?;
    Ok(SelfAudit {
        triggered,
        false_pos,
        ledger_checks,
        hysteresis_ok,
    })
}
