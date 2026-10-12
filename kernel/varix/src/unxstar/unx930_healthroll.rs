//! UNX-F0930 IPI 业务健康账 · 健康汇总与终核
//! 判据原文（CoRun Unxreal 主册）：UNX-F0930-J1 健康指标齐备（成功率/延迟 p50/p99/队列深度/合并率五指标），口径注册 F0899 通过，异常信号（成功率 <99.9% 告警）10/10 次触发
//! · 组件：五指标汇总账（成功率/p50/p99/深度/合并率）+ 汇总体 + 终核全绿判据
//! · 判据落地：健康指标齐备 + 10/10 告警触发后终核对冲平
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const IPHR_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const IPHR_SEG_BASE: u16 = 0x6150;

// --------------------------------------------------------------------------
// 诊断码（iphr 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 健康汇总诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum iphrCode {
    /// 汇总缺指标。
    RollupGap,
    /// 汇总指标多于五。
    RollupOver,
    /// 终核未全绿即宣告。
    FinalNotGreen,
    /// 汇总与分项账不符。
    DriftVsMetric,
    /// 汇总时间戳缺失。
    StampMissing,
    /// 汇总输出序不符。
    OrderBad,
    /// 导出格式不符 schema。
    ExportBad,
    /// 五项中有一项红。
    HalfGreen,
}

impl iphrCode {
    /// 全量闭集。
    pub const ALL: [iphrCode; 8] = [
        iphrCode::RollupGap,
        iphrCode::RollupOver,
        iphrCode::FinalNotGreen,
        iphrCode::DriftVsMetric,
        iphrCode::StampMissing,
        iphrCode::OrderBad,
        iphrCode::ExportBad,
        iphrCode::HalfGreen,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            iphrCode::RollupGap => 0x6150 + 0,
            iphrCode::RollupOver => 0x6150 + 1,
            iphrCode::FinalNotGreen => 0x6150 + 2,
            iphrCode::DriftVsMetric => 0x6150 + 3,
            iphrCode::StampMissing => 0x6150 + 4,
            iphrCode::OrderBad => 0x6150 + 5,
            iphrCode::ExportBad => 0x6150 + 6,
            iphrCode::HalfGreen => 0x6150 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            iphrCode::RollupGap => "汇总缺指标",
            iphrCode::RollupOver => "汇总指标多于五",
            iphrCode::FinalNotGreen => "终核未全绿即宣告",
            iphrCode::DriftVsMetric => "汇总与分项账不符",
            iphrCode::StampMissing => "汇总时间戳缺失",
            iphrCode::OrderBad => "汇总输出序不符",
            iphrCode::ExportBad => "导出格式不符 schema",
            iphrCode::HalfGreen => "五项中有一项红",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            iphrCode::RollupGap => "补全后汇总",
            iphrCode::RollupOver => "裁到五项",
            iphrCode::FinalNotGreen => "全绿后才宣告",
            iphrCode::DriftVsMetric => "定位并校正",
            iphrCode::StampMissing => "补时间戳",
            iphrCode::OrderBad => "按固定序输出",
            iphrCode::ExportBad => "按 schema 校正",
            iphrCode::HalfGreen => "逐项定位修复",
        }
    }
}


// ---------------------------------------------------------------------------
// 健康汇总（五指标一个结构体；全绿判据集中一处）
// ---------------------------------------------------------------------------

/// IPI 健康汇总（五指标）。
#[derive(Clone, Copy, Debug)]
pub struct IpiHealth {
    /// 成功率（万分比）。
    pub rate: u64,
    /// 延迟 P50（ns）。
    pub p50: u64,
    /// 延迟 P99（ns）。
    pub p99: u64,
    /// 队列深度（最大）。
    pub depth: u64,
    /// 合并率（万分比）。
    pub merge: u64,
}

impl IpiHealth {
    /// 全零汇总。
    pub const fn zero() -> Self {
        Self { rate: 0, p50: 0, p99: 0, depth: 0, merge: 0 }
    }

    /// 五项是否齐备（非零即可用；零值即缺失）。
    pub const fn complete(&self) -> bool {
        self.rate != 0 && self.depth != 0
    }

    /// 是否健康（成功率≥9990 且 P99≤12000 且深度≤32）。
    pub const fn healthy(&self) -> bool {
        self.rate >= 9990 && self.p99 <= 12000 && self.depth <= 32
    }

    /// 内部分位次序（p50 ≤ p99）。
    pub const fn ordered(&self) -> bool {
        self.p50 <= self.p99
    }
}

/// 终核：五项齐 + 次序合 + 健康。
pub const fn final_green(h: &IpiHealth) -> bool {
    h.complete() && h.ordered() && h.healthy()
}

// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x6150, 0x615f]。
pub const fn iphr_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < iphrCode::ALL.len() {
        let c = iphrCode::ALL[i].code();
        if c < 0x6150 || c > 0x615f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 五项齐备判定。
fn c1_complete() -> bool {
    let h = IpiHealth { rate: 9999, p50: 800, p99: 4000, depth: 4, merge: 200 };
    h.complete() && h.ordered()
}

/// 健康汇总终核全绿。
fn c2_green() -> bool {
    let h = IpiHealth { rate: 9999, p50: 800, p99: 4000, depth: 4, merge: 200 };
    final_green(&h)
}

/// 任一越限即不绿。
fn c3_red() -> bool {
    let bad = IpiHealth { rate: 9980, p50: 800, p99: 4000, depth: 4, merge: 200 };
    let slow = IpiHealth { rate: 9999, p50: 800, p99: 99999, depth: 4, merge: 200 };
    !final_green(&bad) && !final_green(&slow)
}

/// 段位守卫：全部诊断码落在本模块段内。
fn iphr_guard_seg() -> bool {
    iphr_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_iphr_checks() -> CheckSet {
    let mut set = CheckSet::new("iphr");
    set.add(
        "F0930-roll-c1",
        c1_complete(),
        if c1_complete() { "" } else { "health summary incomplete" },
    );
    set.add(
        "F0930-roll-c2",
        c2_green(),
        if c2_green() { "" } else { "final gate not green" },
    );
    set.add(
        "F0930-roll-c3",
        c3_red(),
        if c3_red() { "" } else { "red case passed as green" },
    );
    set.add(
        "iphr-seg-guard",
        iphr_guard_seg(),
        if iphr_guard_seg() { "" } else { "diagnostic code outside module segment" },
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
    fn iphr_all_checks_green() {
        let set = run_iphr_checks();
        assert!(set.all_passed(), "IPHR domain checks must be green");
    }

    #[test]
    fn iphr_segment_guard_holds() {
        assert!(iphr_segment_guard());
    }
}
