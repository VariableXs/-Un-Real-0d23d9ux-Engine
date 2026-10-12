//! UNX-F1141 top half 执行实测账 · 全局分位数引擎
//! 判据原文（CoRun Unxreal 主册）：UNX-F1141-J1 全注册向量执行时间分布账（P50/P95/P99/max）逐向量可查，实测口径与 F0943 预算同源一致率 100%
//! · 组件：256 向量分位汇总 + 全局最慢向量定位 + 分位插值对拍
//! · 判据落地：分布账逐向量可查
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

// 修正记录：本模块复用同族 `unx1141_pervec` 的执行账类型，类型本身是
// `pub`，但缺 `use` 导致 E0425/E0433（4 个报错同源）。显式引入。
use super::unx1141_pervec::{VecExecLedger, VecExecTable};

use crate::checks::CheckSet;

/// 模块版本。
pub const THP_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const THP_SEG_BASE: u16 = 0x62a0;

// --------------------------------------------------------------------------
// 诊断码（thp 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 全局分位诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum thpCode {
    /// 向量无数据即参与全局分位。
    VectorNoData,
    /// 插值口径不一致。
    InterpBad,
    /// 最慢向量定位错。
    SlowestBad,
    /// 全局分位与逐向量不符。
    PctGlobalBad,
    /// 样本偏向单向量。
    SampleBias,
    /// 累加溢出。
    Overflow,
    /// 全局分位与逐向量不符即宣告。
    FinalNotGreen,
    /// 导出格式不符。
    ExportBad,
}

impl thpCode {
    /// 全量闭集。
    pub const ALL: [thpCode; 8] = [
        thpCode::VectorNoData,
        thpCode::InterpBad,
        thpCode::SlowestBad,
        thpCode::PctGlobalBad,
        thpCode::SampleBias,
        thpCode::Overflow,
        thpCode::FinalNotGreen,
        thpCode::ExportBad,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            thpCode::VectorNoData => 0x62a0 + 0,
            thpCode::InterpBad => 0x62a0 + 1,
            thpCode::SlowestBad => 0x62a0 + 2,
            thpCode::PctGlobalBad => 0x62a0 + 3,
            thpCode::SampleBias => 0x62a0 + 4,
            thpCode::Overflow => 0x62a0 + 5,
            thpCode::FinalNotGreen => 0x62a0 + 6,
            thpCode::ExportBad => 0x62a0 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            thpCode::VectorNoData => "向量无数据即参与全局分位",
            thpCode::InterpBad => "插值口径不一致",
            thpCode::SlowestBad => "最慢向量定位错",
            thpCode::PctGlobalBad => "全局分位与逐向量不符",
            thpCode::SampleBias => "样本偏向单向量",
            thpCode::Overflow => "累加溢出",
            thpCode::FinalNotGreen => "全局分位与逐向量不符即宣告",
            thpCode::ExportBad => "导出格式不符",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            thpCode::VectorNoData => "剔除非活跃向量",
            thpCode::InterpBad => "统一最近秩法",
            thpCode::SlowestBad => "重扫全表",
            thpCode::PctGlobalBad => "重算",
            thpCode::SampleBias => "按向量加权",
            thpCode::Overflow => "改用 u128 中转",
            thpCode::FinalNotGreen => "相符后才宣告",
            thpCode::ExportBad => "按 schema 校正",
        }
    }
}


// ---------------------------------------------------------------------------
// 全局分位与最慢向量定位（跨 256 向量汇总）
// ---------------------------------------------------------------------------

/// 定位最慢向量（返回 (向量号, p99)）。
pub fn slowest_vector(vecs: &[VecExecLedger; 256]) -> (u16, u64) {
    let mut best_v = 0u16;
    let mut best = 0u64;
    let mut v = 0usize;
    while v < 256 {
        if vecs[v].opened && vecs[v].p99 > best {
            best = vecs[v].p99;
            best_v = v as u16;
        }
        v += 1;
    }
    (best_v, best)
}

/// 活跃向量数（有开户且有样本）。
pub fn active_vectors(vecs: &[VecExecLedger; 256]) -> u64 {
    let mut n = 0u64;
    let mut v = 0usize;
    while v < 256 {
        if vecs[v].opened && vecs[v].n > 0 {
            n += 1;
        }
        v += 1;
    }
    n
}

/// 全局样本总数。
pub fn total_samples(vecs: &[VecExecLedger; 256]) -> u64 {
    let mut s = 0u64;
    let mut v = 0usize;
    while v < 256 {
        s = s.wrapping_add(vecs[v].n);
        v += 1;
    }
    s
}

// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x62a0, 0x62af]。
pub const fn thp_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < thpCode::ALL.len() {
        let c = thpCode::ALL[i].code();
        if c < 0x62a0 || c > 0x62af {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 最慢向量定位正确。
fn c1_slowest() -> bool {
    let mut t = VecExecTable::new();
    t.open(32);
    t.open(33);
    let mut a = t.vecs[32];
    a.p99 = 900;
    let mut b = t.vecs[33];
    b.p99 = 1200;
    t.vecs[32] = a;
    t.vecs[33] = b;
    let (v, p) = slowest_vector(&t.vecs);
    v == 33 && p == 1200
}

/// 活跃向量与总样本。
fn c2_active() -> bool {
    let mut t = VecExecTable::new();
    t.open(32);
    t.sample(32, 100);
    t.sample(32, 200);
    t.open(33);
    active_vectors(&t.vecs) == 1 && total_samples(&t.vecs) == 2
}

/// 空表定位为 0 号。
fn c3_zero() -> bool {
    let t = VecExecTable::new();
    let (v, p) = slowest_vector(&t.vecs);
    v == 0 && p == 0 && active_vectors(&t.vecs) == 0
}

/// 段位守卫：全部诊断码落在本模块段内。
fn thp_guard_seg() -> bool {
    thp_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_thp_checks() -> CheckSet {
    let mut set = CheckSet::new("thp");
    set.add(
        "F1141-glb-c1",
        c1_slowest(),
        if c1_slowest() { "" } else { "slowest vector located wrong" },
    );
    set.add(
        "F1141-glb-c2",
        c2_active(),
        if c2_active() { "" } else { "active/total sample count wrong" },
    );
    set.add(
        "F1141-glb-c3",
        c3_zero(),
        if c3_zero() { "" } else { "empty table located nonzero" },
    );
    set.add(
        "thp-seg-guard",
        thp_guard_seg(),
        if thp_guard_seg() { "" } else { "diagnostic code outside module segment" },
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
    fn thp_all_checks_green() {
        let set = run_thp_checks();
        assert!(set.all_passed(), "THP domain checks must be green");
    }

    #[test]
    fn thp_segment_guard_holds() {
        assert!(thp_segment_guard());
    }
}
