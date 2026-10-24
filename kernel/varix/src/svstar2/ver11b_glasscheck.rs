//! VE-F3562 · 深空玻璃符合性检查器 —— 基线逐项对照 + 偏差量化 + 三档分级 + 版本锁定。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3562`
//!
//! # 职责（锚点原文拆解）
//!
//! - **基线逐项对照**（[`GlassChecker`]）：毛玻璃层级 / 光影强度 /
//!   模糊半径三类基线令牌逐项对照。
//! - **偏差量化**：差多少可查（不是"有点不对"而是"差 23 千分比"）。
//! - **三档分级**（[`DeviationGrade`]）：阻断 / 建议 / 观察三档。
//! - **版本锁定**：对照的是哪版基线可查（[`BASELINE_VERSION`]）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 检查失效 | 自检 |
//! | 基线漂移 | 版本锁定 |
//! | 偏差未分级 | 补级 |
//!
//! # 复杂度
//!
//! O(令牌)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E20 段续：6420 起）
// ---------------------------------------------------------------------------

/// 玻璃符合性诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GlassCode {
    /// 检出阻断级偏差。
    BlockingDeviation = 6420,
    /// 基线版本漂移。
    BaselineDrift = 6421,
    /// 偏差未分级。
    UNGraded = 6422,
}

impl GlassCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            GlassCode::BlockingDeviation => "阻断级偏差",
            GlassCode::BaselineDrift => "基线版本漂移",
            GlassCode::UNGraded => "偏差未分级",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, GlassCode::BlockingDeviation | GlassCode::BaselineDrift)
    }
}

// ---------------------------------------------------------------------------
// 二、基线
// ---------------------------------------------------------------------------

/// 基线版本。
pub const BASELINE_VERSION: u32 = 1;

/// 偏差分级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviationGrade {
    /// 阻断（> 200 千分比）。
    Block,
    /// 建议（50–200 千分比）。
    Suggest,
    /// 观察（< 50 千分比）。
    Observe,
}

impl DeviationGrade {
    /// 档短名。
    pub const fn label(self) -> &'static str {
        match self {
            DeviationGrade::Block => "阻断",
            DeviationGrade::Suggest => "建议",
            DeviationGrade::Observe => "观察",
        }
    }
}

/// 基线令牌（三类之一）。
#[derive(Clone, Copy, Debug)]
pub struct GlassToken {
    /// 令牌 id。
    pub id: u32,
    /// 基线值（千分比或像素，量纲由类自定）。
    pub baseline: u32,
    /// 实测值。
    pub measured: u32,
}

impl GlassToken {
    /// 偏差绝对值。
    pub const fn deviation(&self) -> u32 {
        if self.measured > self.baseline {
            self.measured - self.baseline
        } else {
            self.baseline - self.measured
        }
    }

    /// 分级。
    pub const fn grade(&self) -> DeviationGrade {
        let d = self.deviation();
        if d > 200 {
            DeviationGrade::Block
        } else if d >= 50 {
            DeviationGrade::Suggest
        } else {
            DeviationGrade::Observe
        }
    }
}

// ---------------------------------------------------------------------------
// 三、检查器
// ---------------------------------------------------------------------------

/// 检查结果。
#[derive(Clone, Copy, Debug)]
pub struct CheckResult {
    /// 令牌。
    pub token: GlassToken,
    /// 分级。
    pub grade: DeviationGrade,
}

/// 玻璃符合性检查器。
pub struct GlassChecker {
    version: u32,
    results: Vec<CheckResult>,
}

impl GlassChecker {
    /// 新建（绑定基线版本）。
    pub fn new(version: u32) -> Result<Self, GlassCode> {
        if version != BASELINE_VERSION {
            return Err(GlassCode::BaselineDrift);
        }
        Ok(Self {
            version,
            results: Vec::new(),
        })
    }

    /// 逐项对照一枚令牌。
    pub fn check(&mut self, token: GlassToken) {
        self.results.push(CheckResult {
            token,
            grade: token.grade(),
        });
    }

    /// 阻断级偏差数。
    pub fn blocking_count(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.grade == DeviationGrade::Block)
            .count()
    }

    /// 全部偏差数。
    pub fn total_count(&self) -> usize {
        self.results.len()
    }

    /// 版本绑定可查。
    pub fn version(&self) -> u32 {
        self.version
    }
}

// ---------------------------------------------------------------------------
// 四、判据
// ---------------------------------------------------------------------------

/// 玻璃符合性判据集。
pub fn run_glass_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver11b");

    // 1. 版本锁定。
    let ok = GlassChecker::new(BASELINE_VERSION).is_ok();
    let bad = GlassChecker::new(99).is_err();
    chk.add("6420-版本锁定", ok && bad, "");

    // 2. 偏差量化与分级。
    let t1 = GlassToken {
        id: 1,
        baseline: 500,
        measured: 510,
    };
    let t2 = GlassToken {
        id: 2,
        baseline: 500,
        measured: 600,
    };
    let t3 = GlassToken {
        id: 3,
        baseline: 500,
        measured: 800,
    };
    chk.add(
        "6421-三档分级",
        t1.grade() == DeviationGrade::Observe
            && t2.grade() == DeviationGrade::Suggest
            && t3.grade() == DeviationGrade::Block,
        "",
    );

    // 3. 偏差量化正确。
    chk.add("6422-偏差量化", t2.deviation() == 100 && t3.deviation() == 300, "");

    // 4. 阻断计数。
    let mut c = GlassChecker::new(BASELINE_VERSION).ok().unwrap_or(GlassChecker {
        version: BASELINE_VERSION,
        results: Vec::new(),
    });
    c.check(t1);
    c.check(t2);
    c.check(t3);
    chk.add("6423-阻断计数", c.blocking_count() == 1 && c.total_count() == 3, "");

    // 5. 版本可查。
    chk.add("6424-版本可查", c.version() == BASELINE_VERSION, "");

    chk
}
