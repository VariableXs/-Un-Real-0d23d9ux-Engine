//! VE-F3570 · 一致性回归门 —— 第五门 + 规则版本化 + 白名单期限 + 兜底扫描。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3570`
//!
//! # 职责（锚点原文拆解）
//!
//! - **第五门**（[`ConsistencyGate`]）：四门之外的第五道回归门。
//! - **规则版本化**：门禁规则带版本，漂移拦截。
//! - **白名单期限**：误报白名单自动过期。
//! - **兜底扫描**：漏检兜底。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 门禁误报 | 白名单 + 期限 |
//! | 规则漂移 | 版本拦截 |
//! | 漏检 | 兜底 |
//!
//! # 复杂度
//!
//! O(1)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E20 段续：6580 起）
// ---------------------------------------------------------------------------

/// 回归门诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateCode {
    /// 门禁拦截。
    GateBlocked = 6580,
    /// 规则版本漂移。
    RuleDrift = 6581,
    /// 白名单过期。
    WhitelistExpired = 6582,
}

impl GateCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            GateCode::GateBlocked => "一致性门禁拦截",
            GateCode::RuleDrift => "门禁规则版本漂移",
            GateCode::WhitelistExpired => "白名单过期",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, GateCode::GateBlocked | GateCode::RuleDrift)
    }
}

// ---------------------------------------------------------------------------
// 二、规则版本
// ---------------------------------------------------------------------------

/// 门禁规则版本。
pub const GATE_RULE_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// 三、白名单
// ---------------------------------------------------------------------------

/// 白名单条目。
#[derive(Clone, Copy, Debug)]
pub struct ExemptEntry {
    /// 规则 id。
    pub rule: u32,
    /// 剩余天数。
    pub days_left: u32,
}

/// 误报白名单（期限自动过期）。
#[derive(Default)]
pub struct ExemptList {
    entries: Vec<ExemptEntry>,
}

impl ExemptList {
    /// 新建。
    pub fn new() -> Self {
        Self::default()
    }

    /// 加入（7 天期限）。
    pub fn add(&mut self, rule: u32) {
        self.entries.push(ExemptEntry {
            rule,
            days_left: 7,
        });
    }

    /// 过一日。
    pub fn tick(&mut self) {
        for e in self.entries.iter_mut() {
            e.days_left = e.days_left.saturating_sub(1);
        }
        self.entries.retain(|e| e.days_left > 0);
    }

    /// 查豁免。
    pub fn exempts(&self, rule: u32) -> bool {
        self.entries.iter().any(|e| e.rule == rule)
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 四、回归门
// ---------------------------------------------------------------------------

/// 一致性回归门。
pub struct ConsistencyGate {
    version: u32,
    exempts: ExemptList,
}

impl ConsistencyGate {
    /// 新建（版本锁定）。
    pub fn new(version: u32) -> Result<Self, GateCode> {
        if version != GATE_RULE_VERSION {
            return Err(GateCode::RuleDrift);
        }
        Ok(Self {
            version,
            exempts: ExemptList::new(),
        })
    }

    /// 评估一条规则结果（违规码 + 规则 id）。
    /// 白名单在期 → 放行；否则拦截。
    pub fn evaluate(&self, rule_id: u32, violated: bool) -> Result<(), GateCode> {
        if !violated {
            return Ok(());
        }
        if self.exempts.exempts(rule_id) {
            return Ok(());
        }
        Err(GateCode::GateBlocked)
    }

    /// 白名单可变。
    pub fn exempts_mut(&mut self) -> &mut ExemptList {
        &mut self.exempts
    }

    /// 版本可查。
    pub fn version(&self) -> u32 {
        self.version
    }
}

// ---------------------------------------------------------------------------
// 五、判据
// ---------------------------------------------------------------------------

/// 回归门判据集。
pub fn run_gate5_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver11j");

    // 1. 版本锁定。
    chk.add(
        "6580-版本锁定",
        ConsistencyGate::new(GATE_RULE_VERSION).is_ok() && ConsistencyGate::new(99).is_err(),
        "",
    );

    // 2. 无违规放行。
    let g = ConsistencyGate::new(GATE_RULE_VERSION).ok().unwrap_or(ConsistencyGate {
        version: GATE_RULE_VERSION,
        exempts: ExemptList::new(),
    });
    chk.add("6581-无违规过", g.evaluate(1, false).is_ok(), "");

    // 3. 有违规拦截。
    chk.add(
        "6582-违规拦",
        g.evaluate(1, true).err() == Some(GateCode::GateBlocked),
        "",
    );

    // 4. 白名单豁免。
    let mut g2 = ConsistencyGate::new(GATE_RULE_VERSION).ok().unwrap_or(ConsistencyGate {
        version: GATE_RULE_VERSION,
        exempts: ExemptList::new(),
    });
    g2.exempts_mut().add(7);
    chk.add("6583-白名单豁免", g2.evaluate(7, true).is_ok(), "");

    // 5. 白名单过期。
    for _ in 0..8 {
        g2.exempts_mut().tick();
    }
    chk.add(
        "6584-白名单过期",
        g2.evaluate(7, true).err() == Some(GateCode::GateBlocked),
        "",
    );

    // 6. 版本可查。
    chk.add("6585-版本可查", g.version() == GATE_RULE_VERSION, "");

    chk
}
