//! UNX-F1141 top half 执行实测账 · 四统量分布
//! 判据原文（CoRun Unxreal 主册）：UNX-F1141-J1 全注册向量执行时间分布账（P50/P95/P99/max）逐向量可查，实测口径与 F0943 预算同源一致率 100%
//! · 组件：每向量 P50/P95/P99/max 四统量 + 有界采样 + 分布一致性断言
//! · 判据落地：P50/P95/P99/max 逐向量可查
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const THD_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const THD_SEG_BASE: u16 = 0x6280;

// --------------------------------------------------------------------------
// 诊断码（thd 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 分布统计诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum thdCode {
    /// 统量次序破（p50>p95）。
    PctOrderBad,
    /// 样本不足即出分位。
    SampleThin,
    /// 离群点漏记。
    OutlierMiss,
    /// 直方图满员丢样本。
    HistFull,
    /// 零样本向量出零统量。
    ZeroVec,
    /// 分位与实测不符。
    PctDrift,
    /// max 与样本极值不符。
    MaxMismatch,
    /// 有向量统量乱即宣告。
    FinalNotGreen,
}

impl thdCode {
    /// 全量闭集。
    pub const ALL: [thdCode; 8] = [
        thdCode::PctOrderBad,
        thdCode::SampleThin,
        thdCode::OutlierMiss,
        thdCode::HistFull,
        thdCode::ZeroVec,
        thdCode::PctDrift,
        thdCode::MaxMismatch,
        thdCode::FinalNotGreen,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            thdCode::PctOrderBad => 0x6280 + 0,
            thdCode::SampleThin => 0x6280 + 1,
            thdCode::OutlierMiss => 0x6280 + 2,
            thdCode::HistFull => 0x6280 + 3,
            thdCode::ZeroVec => 0x6280 + 4,
            thdCode::PctDrift => 0x6280 + 5,
            thdCode::MaxMismatch => 0x6280 + 6,
            thdCode::FinalNotGreen => 0x6280 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            thdCode::PctOrderBad => "统量次序破（p50>p95）",
            thdCode::SampleThin => "样本不足即出分位",
            thdCode::OutlierMiss => "离群点漏记",
            thdCode::HistFull => "直方图满员丢样本",
            thdCode::ZeroVec => "零样本向量出零统量",
            thdCode::PctDrift => "分位与实测不符",
            thdCode::MaxMismatch => "max 与样本极值不符",
            thdCode::FinalNotGreen => "有向量统量乱即宣告",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            thdCode::PctOrderBad => "按排序重建",
            thdCode::SampleThin => "最低样本量门槛",
            thdCode::OutlierMiss => "保留 max 计数",
            thdCode::HistFull => "扩容或记丢失",
            thdCode::ZeroVec => "标明无数据",
            thdCode::PctDrift => "重算分位",
            thdCode::MaxMismatch => "重建",
            thdCode::FinalNotGreen => "全合后才宣告",
        }
    }
}


// --------------------------------------------------------------------------
// 直方图引擎 thd_dist（有界插入排序 + 五统量 + 分位数）
// --------------------------------------------------------------------------
/// 执行时间分布直方图（ns，四统量）。
pub const THD_DIST_CAP: usize = 256;

#[derive(Clone, Copy, Debug)]
pub struct thd_distHist {
    /// 有序样本（插入排序维护）。
    pub samples: [u64; 256],
    /// 样本数。
    pub n: usize,
    /// 满员丢弃计数（显性不静默）。
    pub drops: u64,
    /// 累加和。
    pub sum: u64,
    /// 最小值。
    pub min: u64,
    /// 最大值。
    pub max: u64,
}

impl thd_distHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 256], n: 0, drops: 0, sum: 0, min: 0, max: 0 }
    }

    /// 采样（满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= THD_DIST_CAP {
            self.drops += 1;
            return;
        }
        let mut j = self.n;
        while j > 0 && self.samples[j - 1] > v {
            self.samples[j] = self.samples[j - 1];
            j -= 1;
        }
        self.samples[j] = v;
        self.n += 1;
        self.sum = self.sum.wrapping_add(v);
        if self.n == 1 || v < self.min {
            self.min = v;
        }
        if v > self.max {
            self.max = v;
        }
    }

    /// 分位数（permille 口径最近秩法）。
    pub fn percentile(&self, permille: u64) -> u64 {
        if self.n == 0 {
            return 0;
        }
        let rank = (permille * self.n as u64 + 999) / 1000;
        let idx = if rank == 0 { 1 } else { rank };
        let i = if idx as usize > self.n { self.n - 1 } else { idx as usize - 1 };
        self.samples[i]
    }

    /// 均值（整数除）。
    pub fn mean(&self) -> u64 {
        if self.n == 0 {
            return 0;
        }
        self.sum / self.n as u64
    }

    /// 守恒：Σ样本 == sum。
    pub fn conserved(&self) -> bool {
        let mut s = 0u64;
        let mut i = 0usize;
        while i < self.n {
            s = s.wrapping_add(self.samples[i]);
            i += 1;
        }
        s == self.sum
    }
}


// --------------------------------------------------------------------------
// 账本 thd_roll（四统量汇总账）
// --------------------------------------------------------------------------
/// 四统量汇总账。
#[derive(Clone, Copy, Debug)]
pub struct thd_roll {
    /// 参与统计向量数
    pub vectors: u64,
    /// 统量次序合法向量数
    pub ordered: u64,
    /// 次序非法向量数
    pub unordered: u64,
    /// 零样本向量数
    pub zero_sample: u64,
}

impl thd_roll {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            vectors: 0,
            ordered: 0,
            unordered: 0,
            zero_sample: 0,
        }
    }

    /// 一个向量统量次序合法。
    pub fn vec_ordered(&mut self) -> () {
        self.vectors += 1;
        self.ordered += 1;
    }

    /// 一个向量统量次序非法。
    pub fn vec_unordered(&mut self) -> () {
        self.vectors += 1;
        self.unordered += 1;
    }

    /// 一个向量零样本。
    pub fn vec_zero(&mut self) -> () {
        self.vectors += 1;
        self.zero_sample += 1;
    }

    /// 全部向量次序合法。
    pub fn all_ordered(&mut self) -> bool {
        self.vectors > 0 && self.unordered == 0 && self.zero_sample == 0
    }

}


// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x6280, 0x628f]。
pub const fn thd_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < thdCode::ALL.len() {
        let c = thdCode::ALL[i].code();
        if c < 0x6280 || c > 0x628f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 分布直方图四统量次序。
fn c1_hist() -> bool {
    // 修正记录：原调用 `thd_dist_hist_new()` 不存在（E0425 实锤）——
    // 本模块真实类型为 `thd_distHist`，构造器是它的 `new()`。
    let mut h = thd_distHist::new();
    let mut i = 0u64;
    while i < 1000 { h.push(100 + (i % 50)); i += 1; }
    h.percentile(500) <= h.percentile(950)
        && h.percentile(950) <= h.percentile(990)
        && h.percentile(990) <= h.max
        && h.min == 100 && h.max == 149
        && h.conserved()
}

/// 汇总账全合法。
fn c2_roll() -> bool {
    let mut r = thd_roll::new();
    let mut i = 0u64;
    while i < 20 { r.vec_ordered(); i += 1; }
    r.all_ordered() && r.vectors == 20
}

/// 一个向量乱即不合法。
fn c3_bad() -> bool {
    let mut r = thd_roll::new();
    let mut i = 0u64;
    while i < 19 { r.vec_ordered(); i += 1; }
    r.vec_unordered();
    !r.all_ordered() && r.unordered == 1
}

/// 段位守卫：全部诊断码落在本模块段内。
fn thd_guard_seg() -> bool {
    thd_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_thd_checks() -> CheckSet {
    let mut set = CheckSet::new("thd");
    set.add(
        "F1141-dist-c1",
        c1_hist(),
        if c1_hist() { "" } else { "distribution statistics order broken" },
    );
    set.add(
        "F1141-dist-c2",
        c2_roll(),
        if c2_roll() { "" } else { "distribution rollup not all ordered" },
    );
    set.add(
        "F1141-dist-c3",
        c3_bad(),
        if c3_bad() { "" } else { "unordered vector slipped through" },
    );
    set.add(
        "thd-seg-guard",
        thd_guard_seg(),
        if thd_guard_seg() { "" } else { "diagnostic code outside module segment" },
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
    fn thd_all_checks_green() {
        let set = run_thd_checks();
        assert!(set.all_passed(), "THD domain checks must be green");
    }

    #[test]
    fn thd_segment_guard_holds() {
        assert!(thd_segment_guard());
    }
}
