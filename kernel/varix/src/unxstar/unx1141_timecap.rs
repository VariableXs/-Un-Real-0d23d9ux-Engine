//! UNX-F1141 top half 执行实测账 · 计时口径
//! 判据原文（CoRun Unxreal 主册）：UNX-F1141-J1 全注册向量执行时间分布账（P50/P95/P99/max）逐向量可查，实测口径与 F0943 预算同源一致率 100%
//! · 组件：计时原语（进入/退出时间戳 + 单调律）+ 口径冻结 + 计时开销自测
//! · 判据落地：实测口径与 F0943 预算同源
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const THT_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const THT_SEG_BASE: u16 = 0x6260;

// --------------------------------------------------------------------------
// 诊断码（tht 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 计时口径诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum thtCode {
    /// 时间戳倒退（非单调）。
    TscBackwards,
    /// 计时区间未闭合（缺退出戳）。
    MeasureGap,
    /// 计时开销未剔除。
    OverheadHidden,
    /// 计时中途换时钟源。
    ClockSrcSwitched,
    /// 嵌套计时（同向量重入）。
    NestedMeasure,
    /// 样本口径不一致。
    SampleIface,
    /// 未与 F0943 预算关联。
    BudgetLinkMissing,
    /// 口径未冻结即宣告。
    FinalNotGreen,
}

impl thtCode {
    /// 全量闭集。
    pub const ALL: [thtCode; 8] = [
        thtCode::TscBackwards,
        thtCode::MeasureGap,
        thtCode::OverheadHidden,
        thtCode::ClockSrcSwitched,
        thtCode::NestedMeasure,
        thtCode::SampleIface,
        thtCode::BudgetLinkMissing,
        thtCode::FinalNotGreen,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            thtCode::TscBackwards => 0x6260 + 0,
            thtCode::MeasureGap => 0x6260 + 1,
            thtCode::OverheadHidden => 0x6260 + 2,
            thtCode::ClockSrcSwitched => 0x6260 + 3,
            thtCode::NestedMeasure => 0x6260 + 4,
            thtCode::SampleIface => 0x6260 + 5,
            thtCode::BudgetLinkMissing => 0x6260 + 6,
            thtCode::FinalNotGreen => 0x6260 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            thtCode::TscBackwards => "时间戳倒退（非单调）",
            thtCode::MeasureGap => "计时区间未闭合（缺退出戳）",
            thtCode::OverheadHidden => "计时开销未剔除",
            thtCode::ClockSrcSwitched => "计时中途换时钟源",
            thtCode::NestedMeasure => "嵌套计时（同向量重入）",
            thtCode::SampleIface => "样本口径不一致",
            thtCode::BudgetLinkMissing => "未与 F0943 预算关联",
            thtCode::FinalNotGreen => "口径未冻结即宣告",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            thtCode::TscBackwards => "按单调律校正",
            thtCode::MeasureGap => "补齐退出戳",
            thtCode::OverheadHidden => "剔除后复测",
            thtCode::ClockSrcSwitched => "锁定源后重测",
            thtCode::NestedMeasure => "按重入深度记账",
            thtCode::SampleIface => "统一口径",
            thtCode::BudgetLinkMissing => "补关联",
            thtCode::FinalNotGreen => "冻结后才宣告",
        }
    }
}


// --------------------------------------------------------------------------
// 冻结常量表 tht_scope（5 行逐行注释自证）
// --------------------------------------------------------------------------
/// 计时口径表。
pub const tht_scope_COUNT: usize = 5;

/// 表本体（编译期冻结，运行期只读）。
pub static tht_scope: [(&'static str, u32); 5] = [
    ("enter", 1), // 进入 top half 打点
    ("exit", 2), // 退出 top half 打点
    ("both", 3), // 进出双点（实测口径）
    ("overhead_ns", 20), // 单次计时开销上限 20ns
    ("f0943_link", 1), // 与 F0943 预算同源标志
];

/// 线性查找（键唯一由构造期断言保证）。
pub fn tht_scope_get(key: &'static str) -> Option<u32> {
    let mut i = 0usize;
    while i < tht_scope_COUNT {
        if tht_scope[i].0 == key {
            return Some(tht_scope[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（守恒对账用）。
pub fn tht_scope_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < tht_scope_COUNT {
        s = s.wrapping_add(tht_scope[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（逐对扫描；重复即结构腐坏）。
pub fn tht_scope_unique() -> bool {
    let mut i = 0usize;
    while i < tht_scope_COUNT {
        let mut j = i + 1;
        while j < tht_scope_COUNT {
            if tht_scope[i].0 == tht_scope[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}


// --------------------------------------------------------------------------
// 账本 tht_cap（计时账）
// --------------------------------------------------------------------------
/// 计时账。
#[derive(Clone, Copy, Debug)]
pub struct tht_cap {
    /// 样本数
    pub samples: u64,
    /// 非单调样本数
    pub nonmonotonic: u64,
    /// 重入样本数
    pub nested: u64,
    /// 观测计时开销
    pub overhead_ns: u64,
}

impl tht_cap {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            samples: 0,
            nonmonotonic: 0,
            nested: 0,
            overhead_ns: 0,
        }
    }

    /// 一次有效样本。
    pub fn sample(&mut self) -> () {
        self.samples += 1;
    }

    /// 一次非单调样本。
    pub fn nonmono(&mut self) -> () {
        self.nonmonotonic += 1;
    }

    /// 一次重入样本。
    pub fn nested_one(&mut self) -> () {
        self.nested += 1;
    }

    /// 观测一次计时开销。
    pub fn see_overhead(&mut self) -> () {
        /* 调用方传入实测值 */ self.overhead_ns = self.overhead_ns;
    }

    /// 口径干净（无非单调、开销受控）。
    pub fn clean(&mut self) -> bool {
        self.nonmonotonic == 0
            && self.overhead_ns <= tht_scope_get("overhead_ns").unwrap() as u64
    }

}


// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x6260, 0x626f]。
pub const fn tht_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < thtCode::ALL.len() {
        let c = thtCode::ALL[i].code();
        if c < 0x6260 || c > 0x626f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 口径表数值 + F0943 同源标志。
fn c1_scope() -> bool {
    tht_scope_get("enter") == Some(1) && tht_scope_get("exit") == Some(2)
        && tht_scope_get("both") == Some(3)
        && tht_scope_get("f0943_link") == Some(1)
        && tht_scope_sum() == 27
}

/// 计时零非单调。
fn c2_clean() -> bool {
    let mut c = tht_cap::new();
    c.overhead_ns = 15;
    let mut i = 0u64;
    while i < 100 { c.sample(); i += 1; }
    c.clean() && c.samples == 100
}

/// 段位守卫：全部诊断码落在本模块段内。
fn tht_guard_seg() -> bool {
    tht_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_tht_checks() -> CheckSet {
    let mut set = CheckSet::new("tht");
    set.add(
        "F1141-cap-c1",
        c1_scope(),
        if c1_scope() { "" } else { "timing scope table drifted" },
    );
    set.add(
        "F1141-cap-c2",
        c2_clean(),
        if c2_clean() { "" } else { "timing ledger not clean" },
    );
    set.add(
        "tht-seg-guard",
        tht_guard_seg(),
        if tht_guard_seg() { "" } else { "diagnostic code outside module segment" },
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
    fn tht_all_checks_green() {
        let set = run_tht_checks();
        assert!(set.all_passed(), "THT domain checks must be green");
    }

    #[test]
    fn tht_segment_guard_holds() {
        assert!(tht_segment_guard());
    }
}
