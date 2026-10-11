//! unxg06c_vcu06_resultnorm —— CGPU-F3206 结果归一化（跨硬件归一/基线对照）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：CGPU-F3206 · 结果归一化（跨硬件归一/基线对照）
//! · 组件：基准结果表 + 跨硬件归一引擎 + 基线对照账 + 归一域快照
//! · 判据落地：跨硬件归一 + 基线对照
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const G06C_VERSION: &str = "g06c-vcu06_resultnorm-v1";

/// 诊断段基址。
pub const G06C_SEG_BASE: u16 = 0x7400;

// --------------------------------------------------------------------------
// 诊断码（g06c 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 结果归一化诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum g06cCode {
    /// 0x7400 基线缺失（无参照硬件）。
    BaselineMissing,
    /// 0x7401 单位非法（口径不一致）。
    UnitBad,
    /// 0x7402 归算溢出（除零/超限）。
    NormOverflow,
    /// 0x7403 跨硬件分歧超阈。
    CrossHwDiv,
    /// 0x7404 缩放因子非法（≤0 或超限）。
    ScaleBad,
    /// 0x7405 归一账破缺。
    LedgerBreak,
    /// 0x7406 归一序违例。
    OrderViolated,
    /// 0x7407 守卫触发。
    GuardTripped,
    /// 0x7408 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x7409 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x740a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x740b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x740c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x740d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl g06cCode {
    /// 全量闭集。
    pub const ALL: [g06cCode; 14] = [
        g06cCode::BaselineMissing,
        g06cCode::UnitBad,
        g06cCode::NormOverflow,
        g06cCode::CrossHwDiv,
        g06cCode::ScaleBad,
        g06cCode::LedgerBreak,
        g06cCode::OrderViolated,
        g06cCode::GuardTripped,
        g06cCode::LedgerBroken,
        g06cCode::SnapshotFingerprint,
        g06cCode::TableBroken,
        g06cCode::IndexMisaligned,
        g06cCode::DuplicateEntry,
        g06cCode::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            g06cCode::BaselineMissing => 0x7400,
            g06cCode::UnitBad => 0x7401,
            g06cCode::NormOverflow => 0x7402,
            g06cCode::CrossHwDiv => 0x7403,
            g06cCode::ScaleBad => 0x7404,
            g06cCode::LedgerBreak => 0x7405,
            g06cCode::OrderViolated => 0x7406,
            g06cCode::GuardTripped => 0x7407,
            g06cCode::LedgerBroken => 0x7408,
            g06cCode::SnapshotFingerprint => 0x7409,
            g06cCode::TableBroken => 0x740a,
            g06cCode::IndexMisaligned => 0x740b,
            g06cCode::DuplicateEntry => 0x740c,
            g06cCode::SegmentGuard => 0x740d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            g06cCode::BaselineMissing => "基线缺失（无参照硬件）",
            g06cCode::UnitBad => "单位非法（口径不一致）",
            g06cCode::NormOverflow => "归算溢出（除零/超限）",
            g06cCode::CrossHwDiv => "跨硬件分歧超阈",
            g06cCode::ScaleBad => "缩放因子非法（≤0 或超限）",
            g06cCode::LedgerBreak => "归一账破缺",
            g06cCode::OrderViolated => "归一序违例",
            g06cCode::GuardTripped => "守卫触发",
            g06cCode::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            g06cCode::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            g06cCode::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            g06cCode::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            g06cCode::DuplicateEntry => "条目重复（同键双写）",
            g06cCode::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            g06cCode::BaselineMissing => "按基线表补配",
            g06cCode::UnitBad => "按单位表校正",
            g06cCode::NormOverflow => "按零基红线拒算",
            g06cCode::CrossHwDiv => "按分歧阈值归因",
            g06cCode::ScaleBad => "按缩放域校正",
            g06cCode::LedgerBreak => "按守恒重建",
            g06cCode::OrderViolated => "按序重排",
            g06cCode::GuardTripped => "检查临界区",
            g06cCode::LedgerBroken => "按守恒律重建台账后重跑",
            g06cCode::SnapshotFingerprint => "按防篡改规程重建快照",
            g06cCode::TableBroken => "按语义规则重建常量表",
            g06cCode::IndexMisaligned => "按声明序归一化后重跑",
            g06cCode::DuplicateEntry => "合并同键条目为末次生效",
            g06cCode::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// 跨硬件归一（以基线硬件为标尺：相对值归算 + 单位口径表）
// ---------------------------------------------------------------------------

/// 单位口径（闭集四口径：FPS / ms / 分 / 百分比）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetricUnit {
    /// 帧每秒。
    Fps,
    /// 毫秒。
    Ms,
    /// 评分分。
    Score,
    /// 千分比。
    Permille,
}

impl MetricUnit {
    /// 全量闭集（四口径）。
    pub const ALL: [MetricUnit; 4] = [MetricUnit::Fps, MetricUnit::Ms, MetricUnit::Score, MetricUnit::Permille];

    /// 索引。
    pub const fn index(self) -> usize {
        match self {
            MetricUnit::Fps => 0,
            MetricUnit::Ms => 1,
            MetricUnit::Score => 2,
            MetricUnit::Permille => 3,
        }
    }

    /// 单位名。
    pub const fn en(self) -> &'static str {
        match self {
            MetricUnit::Fps => "fps",
            MetricUnit::Ms => "ms",
            MetricUnit::Score => "score",
            MetricUnit::Permille => "permille",
        }
    }
}

/// 原始测量（值 + 单位 + 硬件标签）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawMeasure {
    /// 原始值（单位口径内的整数标量）。
    pub value: u64,
    /// 单位。
    pub unit: MetricUnit,
    /// 硬件标识（0=基线）。
    pub hw: u32,
}

/// 归一化结果（相对基线的千分比 + 原始值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Normed {
    /// 归一值（相对基线的千分比；1000 = 持平）。
    pub permyriad: u64,
    /// 是否除以零被拒。
    pub rejected: bool,
}

/// 归一化引擎（以基线硬件的同单位测量为标尺）。
pub const fn normalize(cur: &RawMeasure, baseline: &RawMeasure) -> Normed {
    if baseline.value == 0 {
        return Normed { permyriad: 0, rejected: true };
    }
    if cur.unit.index() != baseline.unit.index() {
        // 单位口径不一致：拒算（不可比口径）。"
        return Normed { permyriad: 0, rejected: true };
    }
    Normed {
        permyriad: (cur.value * 1000) / baseline.value,
        rejected: false,
    }
}

/// 缩放因子合法性（1..=10000 千分比）。
pub const fn scale_ok(permyriad: u64) -> bool {
    permyriad >= 1 && permyriad <= 10000
}

/// 跨硬件分歧阈值（±20% 即 800..1200 之外判分歧）。
pub const CROSS_HW_PM_LO: u64 = 800;
pub const CROSS_HW_PM_HI: u64 = 1200;

/// 分歧判定。
pub const fn cross_hw_diverged(n: &Normed) -> bool {
    !n.rejected && (n.permyriad < CROSS_HW_PM_LO || n.permyriad > CROSS_HW_PM_HI)
}

/// 归一账本（基线对照统计）。
#[derive(Clone, Copy, Debug)]
pub struct NormBook {
    /// 已归算次数。
    pub normed: u64,
    /// 拒算次数（零基/口径异）。
    pub rejected: u64,
    /// 分歧次数。
    pub diverged: u64,
}

impl NormBook {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { normed: 0, rejected: 0, diverged: 0 }
    }

    /// 归算一次。
    pub fn apply(&mut self, cur: &RawMeasure, base: &RawMeasure) -> Normed {
        let n = normalize(cur, base);
        if n.rejected {
            self.rejected += 1;
        } else {
            self.normed += 1;
            if cross_hw_diverged(&n) {
                self.diverged += 1;
            }
        }
        n
    }

    /// 归一完成率（normed + rejected == offered）。
    pub fn conserved(&self, offered: u64) -> bool {
        self.normed + self.rejected == offered
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x7400, 0x740f]。
pub const fn g06c_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < g06cCode::ALL.len() {
        let c = g06cCode::ALL[i].code();
        if c < 0x7400 || c > 0x740f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 段位守卫：全部诊断码落在本模块段内。
fn g06c_guard_seg() -> bool {
    g06c_segment_guard()
}

/// 归一引擎（基线 60fps → 90fps = 1500 千分比 + 拒算路径）。
fn check_c1_normalize() -> bool {
    let base = RawMeasure { value: 60, unit: MetricUnit::Fps, hw: 0 };
    let cur = RawMeasure { value: 90, unit: MetricUnit::Fps, hw: 1 };
    let n = normalize(&cur, &base);
    let mut ok = !n.rejected && n.permyriad == 1500;
    if ok {
        ok = cross_hw_diverged(&n);
    }
    if ok {
        // 零基拒算。
        let zero = RawMeasure { value: 0, unit: MetricUnit::Fps, hw: 0 };
        ok = normalize(&cur, &zero).rejected;
    }
    if ok {
        // 口径异拒算。
        let ms = RawMeasure { value: 90, unit: MetricUnit::Ms, hw: 1 };
        ok = normalize(&ms, &base).rejected;
    }
    ok
}

/// 归一账本（守恒 + 分歧统计 + 缩放域）。
fn check_c2_book() -> bool {
    let mut bk = NormBook::new();
    let base = RawMeasure { value: 100, unit: MetricUnit::Score, hw: 0 };
    let near = RawMeasure { value: 105, unit: MetricUnit::Score, hw: 1 };
    let far = RawMeasure { value: 200, unit: MetricUnit::Score, hw: 2 };
    let zero = RawMeasure { value: 0, unit: MetricUnit::Score, hw: 0 };
    let _ = bk.apply(&near, &base);
    let _ = bk.apply(&far, &base);
    let _ = bk.apply(&near, &zero);
    let mut ok = bk.conserved(3) && bk.normed == 2 && bk.rejected == 1;
    if ok {
        // 105% 未超阈，200% 超阈。
        ok = bk.diverged == 1;
    }
    if ok {
        ok = scale_ok(1000) && !scale_ok(0) && !scale_ok(20000);
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_g06c_checks() -> CheckSet {
    let mut set = CheckSet::new("G06C");
    set.add(
        "g06c-seg-guard",
        g06c_guard_seg(),
        if g06c_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "g06c-c1-normalize",
        check_c1_normalize(),
        if check_c1_normalize() { "" } else { "result normalization mismatch" },
    );
    set.add(
        "g06c-c2-book",
        check_c2_book(),
        if check_c2_book() { "" } else { "norm book mismatch" },
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（域聚合测试在 mod.rs——此处只放本模块纯函数级断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g06c_all_checks_green() {
        let set = run_g06c_checks();
        assert!(set.all_passed(), "G06C domain checks must be green");
    }

    #[test]
    fn g06c_segment_guard_holds() {
        assert!(g06c_segment_guard());
    }
}
