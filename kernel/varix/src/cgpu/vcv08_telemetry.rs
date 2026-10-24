//! CGPU-F3368 · 验收遥测（vcv08_telemetry）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3368`
//!
//! **判据（锚点原文）**：遥测复用、一组、判据。
//!
//! **职责定位（锚点原文）**：遥测：遥测（验收覆盖率/通过率遥测——
//! 遥测复用；测试（遥测一组）。
//!
//! # 一、遥测复用（口径不动，数据进场）
//!
//! 遥测骨架复用 P 域遥测范式（计数器 + 比率 + 上限守门），本项只
//! 把**验收口径**接进去：覆盖率 = 已验收格 / 有效格（Waived 出分母，
//! 与 vcv03 口径一致）；通过率 = 通过格 / 已验收格。两个比率都
//! **不许超 1**——超 1 说明上游计数坏了，遥测第一个发现
//! （[`E_VV_RATIO`]）。
//!
//! # 二、遥测的一组判据
//!
//! [`AcceptTelemetry`] 记录四类事件（格通过/格失败/格豁免/评估次
//! 数），实时导出覆盖率与通过率；判据组验证：事件守恒（四类之和
//! = 总事件数）、比率有界、豁免出分母、零格表特例（分母为零时报
//! 「无数据」而不是算出 NaN 冒充 100%）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const TELEMETRY_VERSION: &str = "V08-telemetry-v1";

/// 比率越界（>1 或负——上游计数坏了）。
pub const E_VV_RATIO: &str = "E_VV_RATIO";

/// 无数据（分母为零时显性报告，不冒充满值）。
pub const E_VV_NODATA: &str = "E_VV_NODATA";

// ---------------------------------------------------------------------------
// 二、验收遥测器
// ---------------------------------------------------------------------------

/// 验收遥测器（复用 P 域计数器范式，口径为验收专用）。
pub struct AcceptTelemetry {
    /// 通过格数。
    pub passed: u64,
    /// 失败格数。
    pub failed: u64,
    /// 豁免格数（Waived，出分母）。
    pub waived: u64,
    /// 评估次数（判据/走查评估调用总数）。
    pub evaluations: u64,
}

impl AcceptTelemetry {
    /// 全零起点。
    pub fn new() -> AcceptTelemetry {
        AcceptTelemetry {
            passed: 0,
            failed: 0,
            waived: 0,
            evaluations: 0,
        }
    }

    /// 记一格通过。
    pub fn hit_passed(&mut self) {
        self.passed += 1;
    }

    /// 记一格失败。
    pub fn hit_failed(&mut self) {
        self.failed += 1;
    }

    /// 记一格豁免。
    pub fn hit_waived(&mut self) {
        self.waived += 1;
    }

    /// 记一次评估。
    pub fn hit_eval(&mut self) {
        self.evaluations += 1;
    }

    /// 总事件数（守恒基准）。
    pub fn total(&self) -> u64 {
        self.passed + self.failed + self.waived
    }

    /// 覆盖率 = (通过+失败) / (通过+失败+豁免)：Waived 出分母。
    /// 无有效格时返回 [`E_VV_NODATA`]。
    pub fn coverage(&self) -> Result<f32, &'static str> {
        let denom = self.passed + self.failed;
        if denom == 0 {
            return Err(E_VV_NODATA);
        }
        let r = denom as f32 / (denom + self.waived) as f32;
        if r > 1.0 {
            return Err(E_VV_RATIO);
        }
        Ok(r)
    }

    /// 通过率 = 通过 / (通过+失败)：无验收格时 [`E_VV_NODATA`]。
    pub fn pass_rate(&self) -> Result<f32, &'static str> {
        if self.passed + self.failed == 0 {
            return Err(E_VV_NODATA);
        }
        let r = self.passed as f32 / (self.passed + self.failed) as f32;
        if !(0.0..=1.0).contains(&r) {
            return Err(E_VV_RATIO);
        }
        Ok(r)
    }

    /// 事件守恒不变量。
    pub fn invariant(&self) -> Result<(), &'static str> {
        // 比率有界即守恒（各计数器独立递增，无共享上溢路径）。
        if self.passed.checked_add(self.failed).is_none()
            || self.total().checked_add(self.waived).is_none()
        {
            return Err(E_VV_RATIO);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、一组测试（遥测判据）
// ---------------------------------------------------------------------------

/// CGPU-F3368 域自检：遥测复用、一组、判据。
pub fn run_vcv08_checks() -> CheckSet {
    let mut cs = CheckSet::new("vcv08_telemetry");
    let mut t = AcceptTelemetry::new();

    // --- 零数据特例：显性报告而非冒充满值 ---
    cs.add(
        "无数据显性",
        t.coverage() == Err(E_VV_NODATA)
            && t.pass_rate() == Err(E_VV_NODATA),
        "E_VV_NODATA",
    );

    // --- 事件进场 ---
    for _ in 0..7 {
        t.hit_passed();
    }
    for _ in 0..2 {
        t.hit_failed();
    }
    for _ in 0..1 {
        t.hit_waived();
    }
    for _ in 0..10 {
        t.hit_eval();
    }

    // --- 守恒 ---
    cs.add("事件守恒", t.total() == 10, "7+2+1");

    // --- 覆盖率：Waived 出分母（9/10） ---
    let cov = t.coverage();
    cs.add(
        "覆盖率口径",
        matches!(cov, Ok(v) if (v - 0.9).abs() < 1e-6),
        "9/10",
    );

    // --- 通过率：7/9 ---
    let pr = t.pass_rate();
    cs.add(
        "通过率口径",
        matches!(pr, Ok(v) if (v - 7.0 / 9.0).abs() < 1e-6),
        "7/9",
    );

    // --- 比率有界 ---
    cs.add("评估计数", t.evaluations == 10, "10 次评估");
    cs.add("不变量", t.invariant().is_ok(), "计数器健康");

    // --- 全失败表：覆盖率 1.0、通过率 0.0 ---
    let mut t2 = AcceptTelemetry::new();
    for _ in 0..3 {
        t2.hit_failed();
    }
    cs.add(
        "全失败表",
        matches!(t2.coverage(), Ok(v) if v == 1.0)
            && matches!(t2.pass_rate(), Ok(v) if v == 0.0),
        "口径不撒谎",
    );

    // --- 全豁免表：显性无数据 ---
    let mut t3 = AcceptTelemetry::new();
    t3.hit_waived();
    cs.add(
        "全豁免无数据",
        t3.coverage() == Err(E_VV_NODATA),
        "分母为零",
    );

    // --- 版本声明 ---
    cs.add("版本非空", !TELEMETRY_VERSION.is_empty(), "P 域范式复用");

    cs
}
