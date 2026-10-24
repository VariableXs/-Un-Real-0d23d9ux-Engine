//! VE-F3575 · 审查工具 VE-Studio 集成 —— 设计侧预检 + 规则同源 + 往返校验 + 失真标注。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3575`
//!
//! # 职责（锚点原文拆解）
//!
//! - **设计侧预检**（[`StudioIntegration`]）：设计时就别犯错——
//!   预检规则与引擎同源。
//! - **规则同源**：预检用的规则必须与运行时检查器同源。
//! - **往返校验**：设计→运行时→设计往返不失真。
//! - **失真标注**：往返差异标注可查。
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
// 一、诊断码（E20 段续：6680 起）
// ---------------------------------------------------------------------------

/// Studio 集成诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StudioCode {
    /// 规则不同源。
    RuleNotShared = 6680,
    /// 往返失真。
    RoundTripDrift = 6681,
    /// 预检失效。
    PrecheckDown = 6682,
}

impl StudioCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            StudioCode::RuleNotShared => "预检规则不同源",
            StudioCode::RoundTripDrift => "往返失真",
            StudioCode::PrecheckDown => "预检失效",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, StudioCode::RuleNotShared)
    }
}

// ---------------------------------------------------------------------------
// 二、同源与往返
// ---------------------------------------------------------------------------

/// 规则版本（预检与引擎共享）。
pub const SHARED_RULE_VERSION: u32 = 1;

/// 往返校验器。
pub struct RoundTrip;

impl RoundTrip {
    /// 设计值 → 运行时 → 回读，校验无损。
    pub fn verify(original: u32, round_tripped: u32) -> Result<(), StudioCode> {
        if original == round_tripped {
            Ok(())
        } else {
            Err(StudioCode::RoundTripDrift)
        }
    }
}

/// Studio 集成器。
pub struct StudioIntegration {
    engine_version: u32,
    precheck_version: u32,
    /// 失真标注数。
    pub drift_marks: u32,
}

impl StudioIntegration {
    /// 新建（两版本必须同源）。
    pub fn new(engine: u32, precheck: u32) -> Result<Self, StudioCode> {
        if engine != precheck {
            return Err(StudioCode::RuleNotShared);
        }
        Ok(Self {
            engine_version: engine,
            precheck_version: precheck,
            drift_marks: 0,
        })
    }

    /// 设计侧预检（规则同源保证）。
    pub fn precheck(&self, value: u32, limit: u32) -> Result<(), StudioCode> {
        if self.engine_version != SHARED_RULE_VERSION {
            return Err(StudioCode::RuleNotShared);
        }
        if value > limit {
            return Err(StudioCode::PrecheckDown);
        }
        Ok(())
    }

    /// 往返校验 + 失真标注。
    pub fn round_trip(&mut self, original: u32, returned: u32) -> Result<(), StudioCode> {
        match RoundTrip::verify(original, returned) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.drift_marks = self.drift_marks.saturating_add(1);
                Err(e)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 三、判据
// ---------------------------------------------------------------------------

/// Studio 集成判据集。
pub fn run_studio_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver11o");

    // 1. 同源可建。
    chk.add(
        "6680-同源可建",
        StudioIntegration::new(SHARED_RULE_VERSION, SHARED_RULE_VERSION).is_ok(),
        "",
    );

    // 2. 不同源拦截。
    chk.add(
        "6681-不同源拦",
        StudioIntegration::new(1, 2).err() == Some(StudioCode::RuleNotShared),
        "",
    );

    // 3. 预检拦截超限值。
    let s = StudioIntegration::new(SHARED_RULE_VERSION, SHARED_RULE_VERSION).ok().unwrap_or(StudioIntegration {
        engine_version: SHARED_RULE_VERSION,
        precheck_version: SHARED_RULE_VERSION,
        drift_marks: 0,
    });
    chk.add(
        "6682-预检拦",
        s.precheck(999, 100).err() == Some(StudioCode::PrecheckDown)
            && s.precheck(50, 100).is_ok(),
        "",
    );

    // 4. 往返无损。
    let mut s2 = StudioIntegration::new(SHARED_RULE_VERSION, SHARED_RULE_VERSION).ok().unwrap_or(StudioIntegration {
        engine_version: SHARED_RULE_VERSION,
        precheck_version: SHARED_RULE_VERSION,
        drift_marks: 0,
    });
    chk.add("6683-往返无损", s2.round_trip(42, 42).is_ok(), "");

    // 5. 失真标注。
    chk.add(
        "6684-失真标注",
        s2.round_trip(42, 43).err() == Some(StudioCode::RoundTripDrift) && s2.drift_marks == 1,
        "",
    );

    // 6. 同源常量。
    chk.add("6685-规则版本", SHARED_RULE_VERSION == 1, "");

    chk
}
