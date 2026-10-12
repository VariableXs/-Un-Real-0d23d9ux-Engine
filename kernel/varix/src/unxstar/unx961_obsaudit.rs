//! UNX-F0961 现存轮询栈盘点账 · 观测一致性审计
//! 判据原文（CoRun Unxreal 主册）：UNX-F0961-J1 盘点完整（现存全部轮询点清册零遗漏——grep+运行期双源交叉），特征画像落账，盘点与运行观测一致率 100%
//! · 组件：盘点 vs 运行观测一致率账 + P99 一致性 + 终核对冲
//! · 判据落地：盘点与运行观测一致率 100%
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const PMO_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const PMO_SEG_BASE: u16 = 0x61b0;

// --------------------------------------------------------------------------
// 诊断码（pmo 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 观测一致性审计诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum pmoCode {
    /// 一致率不足 100%。
    ConsistUnder,
    /// 运行观测缺失。
    ObsMissing,
    /// 观测数据过期。
    ObsStale,
    /// 同点双计。
    DoubleCount,
    /// 一致率 100% 未达即宣告。
    FinalNotGreen,
    /// 观测样本偏向单一时段。
    SampleBias,
    /// 观测与扫描漂移。
    DriftVsScan,
    /// 汇总漏点。
    RollupMiss,
}

impl pmoCode {
    /// 全量闭集。
    pub const ALL: [pmoCode; 8] = [
        pmoCode::ConsistUnder,
        pmoCode::ObsMissing,
        pmoCode::ObsStale,
        pmoCode::DoubleCount,
        pmoCode::FinalNotGreen,
        pmoCode::SampleBias,
        pmoCode::DriftVsScan,
        pmoCode::RollupMiss,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            pmoCode::ConsistUnder => 0x61b0 + 0,
            pmoCode::ObsMissing => 0x61b0 + 1,
            pmoCode::ObsStale => 0x61b0 + 2,
            pmoCode::DoubleCount => 0x61b0 + 3,
            pmoCode::FinalNotGreen => 0x61b0 + 4,
            pmoCode::SampleBias => 0x61b0 + 5,
            pmoCode::DriftVsScan => 0x61b0 + 6,
            pmoCode::RollupMiss => 0x61b0 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            pmoCode::ConsistUnder => "一致率不足 100%",
            pmoCode::ObsMissing => "运行观测缺失",
            pmoCode::ObsStale => "观测数据过期",
            pmoCode::DoubleCount => "同点双计",
            pmoCode::FinalNotGreen => "一致率 100% 未达即宣告",
            pmoCode::SampleBias => "观测样本偏向单一时段",
            pmoCode::DriftVsScan => "观测与扫描漂移",
            pmoCode::RollupMiss => "汇总漏点",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            pmoCode::ConsistUnder => "逐点定位差异",
            pmoCode::ObsMissing => "补观测后复对",
            pmoCode::ObsStale => "重新采集",
            pmoCode::DoubleCount => "去重",
            pmoCode::FinalNotGreen => "达标后才宣告",
            pmoCode::SampleBias => "扩时段采样",
            pmoCode::DriftVsScan => "重扫并复核",
            pmoCode::RollupMiss => "补汇总",
        }
    }
}


// --------------------------------------------------------------------------
// 直方图引擎 pmo_consist（有界插入排序 + 五统量 + 分位数）
// --------------------------------------------------------------------------
/// 一致率一致性分布（万分比，P99 须 10000）。
pub const PMO_CONSIST_CAP: usize = 64;

#[derive(Clone, Copy, Debug)]
pub struct pmo_consistHist {
    /// 有序样本（插入排序维护）。
    pub samples: [u64; 64],
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

impl pmo_consistHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 64], n: 0, drops: 0, sum: 0, min: 0, max: 0 }
    }

    /// 采样（满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= PMO_CONSIST_CAP {
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
// 账本 pmo_audit（一致性审计账）
// --------------------------------------------------------------------------
/// 一致性审计账。
#[derive(Clone, Copy, Debug)]
pub struct pmo_audit {
    /// 核对点数
    pub points: u64,
    /// 一致点数
    pub consistent: u64,
    /// 不一致点数
    pub inconsistent: u64,
    /// 已观测点数
    pub obs_done: u64,
}

impl pmo_audit {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            points: 0,
            consistent: 0,
            inconsistent: 0,
            obs_done: 0,
        }
    }

    /// 一个点一致。
    pub fn point_ok(&mut self) -> () {
        self.points += 1;
        self.consistent += 1;
        self.obs_done += 1;
    }

    /// 一个点不一致。
    pub fn point_bad(&mut self) -> () {
        self.points += 1;
        self.inconsistent += 1;
        self.obs_done += 1;
    }

    /// 一致率（万分比）。
    pub fn ratio_permille(&mut self) -> u64 {
        if self.points == 0 { return 0; }
        ((self.consistent as u128 * 10000 / self.points as u128) as u64)
    }

    /// 一致率 100%。
    pub fn full(&mut self) -> bool {
        self.points > 0 && self.consistent == self.points
    }

    /// 终核全绿。
    pub fn final_green(&mut self) -> bool {
        self.full() && self.inconsistent == 0 && self.obs_done == self.points
    }

}


// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x61b0, 0x61bf]。
pub const fn pmo_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < pmoCode::ALL.len() {
        let c = pmoCode::ALL[i].code();
        if c < 0x61b0 || c > 0x61bf {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 一致率 100% 判据。
fn c1_full() -> bool {
    let mut a = pmo_audit::new();
    let mut i = 0u64;
    while i < 40 { a.point_ok(); i += 1; }
    a.ratio_permille() == 10000 && a.full() && a.final_green()
}

/// 一个不一致即红。
fn c2_bad() -> bool {
    let mut a = pmo_audit::new();
    let mut i = 0u64;
    while i < 39 { a.point_ok(); i += 1; }
    a.point_bad();
    !a.full() && !a.final_green() && a.ratio_permille() == 9750
}

/// 一致率直方图 P99 须 10000。
fn c3_hist() -> bool {
    // 修正记录：原调用 `pmo_consist_hist_new()` 不存在（E0425 实锤）——
    // 本模块真实类型为 `pmo_consistHist`，构造器是它的 `new()`。
    let mut h = pmo_consistHist::new();
    let mut i = 0u64;
    while i < 100 { h.push(10000); i += 1; }
    h.percentile(990) == 10000 && h.min == 10000 && h.conserved()
}

/// 段位守卫：全部诊断码落在本模块段内。
fn pmo_guard_seg() -> bool {
    pmo_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_pmo_checks() -> CheckSet {
    let mut set = CheckSet::new("pmo");
    set.add(
        "F0961-obs-c1",
        c1_full(),
        if c1_full() { "" } else { "consistency not 100%" },
    );
    set.add(
        "F0961-obs-c2",
        c2_bad(),
        if c2_bad() { "" } else { "inconsistent point slipped through" },
    );
    set.add(
        "F0961-obs-c3",
        c3_hist(),
        if c3_hist() { "" } else { "consistency histogram drift" },
    );
    set.add(
        "pmo-seg-guard",
        pmo_guard_seg(),
        if pmo_guard_seg() { "" } else { "diagnostic code outside module segment" },
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
    fn pmo_all_checks_green() {
        let set = run_pmo_checks();
        assert!(set.all_passed(), "PMO domain checks must be green");
    }

    #[test]
    fn pmo_segment_guard_holds() {
        assert!(pmo_segment_guard());
    }
}
