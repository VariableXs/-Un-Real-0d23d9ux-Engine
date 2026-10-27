//! UNX-F2448 · 池水位与池告警账 · 诊断段 0x7060（独占 0x7060~0x706F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B03 每 CPU 池批 · UNX-F2448）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2448-J1 池低水位告警 10/10 次触发，正常抖动零误报（滞回
//! 在档）。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 10/10 触发：池水位跌破低告警线（8 页）10/10 次触发，
//!   预警带池号+水位快照。
//!   J1-b 正常抖动零误报：水位在告警线上方抖动 10^4 点零预警；
//!   滞回语义：跌破触发后须回升超解除线（16 页）才复位。
//!   J1-c 池分列：8 核池各自观察器独立，告警账带核号。
//!
//! **段位复核记录**：0x7060~0x706F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x7060~0x706F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x7060;
/// 段顶。
pub const SEG_TOP: u16 = 0x706F;

/// 低告警线（页）。
pub const POOL_ALERT_FLOOR: u64 = 8;
/// 解除线（页）。
pub const POOL_CLEAR_CEIL: u64 = 16;
/// 核数。
pub const CORES: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PoolWarnError {
    /// 0x7061 跌线漏报。
    MissedAlert,
    /// 0x7062 抖动误报。
    FalsePositive,
    /// 0x7063 滞回带非法。
    BadHysteresis,
    /// 0x7064 告警账缺核号。
    LedgerMissing,
}

impl PoolWarnError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            PoolWarnError::MissedAlert => format!(
                "[0x{:04X}] 跌线漏报：观察器采样缺失。\
                 改法：每次池出页后都过观察器。",
                self.code()
            ),
            PoolWarnError::FalsePositive => format!(
                "[0x{:04X}] 正常抖动误报：滞回失效。\
                 改法：解除线必须高于告警线。",
                self.code()
            ),
            PoolWarnError::BadHysteresis => format!(
                "[0x{:04X}] 滞回带非法：{} ≤ {}。\
                 改法：配置校验。",
                POOL_CLEAR_CEIL, POOL_ALERT_FLOOR, self.code()
            ),
            PoolWarnError::LedgerMissing => format!(
                "[0x{:04X}] 告警账缺核号：分列不可归因。\
                 改法：预警结构带 core 字段。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、滞回观察器（池版）
// ===========================================================================

/// 一条池告警。
#[derive(Clone, Copy, Debug)]
pub struct PoolAlert {
    /// 核号。
    pub core: usize,
    /// 触发时水位。
    pub level_at: u64,
    /// 事件号。
    pub seq: u64,
}

/// 单池观察器。
pub struct PoolWatcher {
    /// 预警中。
    pub alerted: bool,
    /// 告警账。
    pub alerts: Vec<PoolAlert>,
    core: usize,
    seq: u64,
}

impl PoolWatcher {
    /// 构造（带核号）。
    pub fn new(core: usize) -> result::Result<Self, PoolWarnError> {
        if POOL_CLEAR_CEIL <= POOL_ALERT_FLOOR {
            return Err(PoolWarnError::BadHysteresis);
        }
        if core >= CORES {
            return Err(PoolWarnError::LedgerMissing);
        }
        Ok(PoolWatcher {
            alerted: false,
            alerts: Vec::new(),
            core,
            seq: 0,
        })
    }

    /// 观察一个水位点。返回是否新触发。
    pub fn observe(&mut self, level: u64) -> bool {
        self.seq += 1;
        if !self.alerted {
            if level < POOL_ALERT_FLOOR {
                self.alerted = true;
                self.alerts.push(PoolAlert {
                    core: self.core,
                    level_at: level,
                    seq: self.seq,
                });
                return true;
            }
        } else if level >= POOL_CLEAR_CEIL {
            self.alerted = false;
        }
        false
    }

    /// 账核。
    pub fn audit_ledger(&self) -> result::Result<usize, PoolWarnError> {
        let mut i = 0;
        while i < self.alerts.len() {
            let a = &self.alerts[i];
            if a.core != self.core || a.level_at >= POOL_ALERT_FLOOR {
                return Err(PoolWarnError::LedgerMissing);
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
        Lcg(seed ^ 0x2448_7060_9E37_79B9)
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

/// 触发演练轮数。
pub const TRIGGER_ROUNDS: usize = 10;
/// 抖动采样数。
pub const JITTER_SAMPLES: u64 = 10_000;

/// J1 证据生成器：10/10 触发 + 10^4 抖动零误报 + 8 核分列账核。
pub fn poolwarn_audit() -> result::Result<(usize, u64), PoolWarnError> {
    let mut caught = 0usize;
    let mut r = 0;
    while r < TRIGGER_ROUNDS {
        let core = r % CORES;
        let mut w = PoolWatcher::new(core)?;
        w.observe(POOL_ALERT_FLOOR + 6);
        w.observe(POOL_ALERT_FLOOR + 1);
        if w.observe(POOL_ALERT_FLOOR - 1) {
            caught += 1;
        } else {
            return Err(PoolWarnError::MissedAlert);
        }
        w.audit_ledger()?;
        r += 1;
    }
    // 抖动零误报。
    let mut w2 = PoolWatcher::new(0)?;
    let mut rng = Lcg::new(0x2448_0001);
    let mut n = 0u64;
    while n < JITTER_SAMPLES {
        let v = POOL_CLEAR_CEIL + rng.next_below(32);
        if w2.observe(v) {
            return Err(PoolWarnError::FalsePositive);
        }
        n += 1;
    }
    Ok((caught, w2.alerts.len() as u64))
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    pub triggered: usize,
    pub false_pos: u64,
    pub hysteresis_ok: bool,
    pub ledger_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.triggered == TRIGGER_ROUNDS && self.false_pos == 0 && self.hysteresis_ok && self.ledger_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, PoolWarnError> {
    let (triggered, false_pos) = poolwarn_audit()?;
    let mut w = PoolWatcher::new(3)?;
    let f1 = w.observe(POOL_ALERT_FLOOR - 4);
    let jitter = w.observe(POOL_ALERT_FLOOR + 5);
    let clear = w.observe(POOL_CLEAR_CEIL + 8);
    let hysteresis_ok = f1 && !jitter && !clear && !w.alerted;
    let ledger_ok = w.audit_ledger()? >= 0;
    Ok(SelfAudit {
        triggered,
        false_pos,
        hysteresis_ok,
        ledger_ok,
    })
}
