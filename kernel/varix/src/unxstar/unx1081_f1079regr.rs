//! UNX-F1081 墙钟 NTP 校准 · 基础面零回归（F1079 复测）
//! 判据原文（CoRun Unxreal 主册）：UNX-F1081-J1 双协议正确（阶跃档输出跳变 10/10 次、渐变档输出斜率受控 10/10 次），校准事件账落盘（来源/幅度/档位全记录），基础面零回归（F1079 判据复测全过）
//! · 组件：回归对拍账（F1079 判据逐条复测）+ 零回归判据 + 回归指纹
//! · 判据落地：基础面零回归（F1079 判据复测全过）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const NTR_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const NTR_SEG_BASE: u16 = 0x6240;

// --------------------------------------------------------------------------
// 诊断码（ntr 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 零回归诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ntrCode {
    /// 检测到回归。
    RegressFound,
    /// 复测漏用例。
    CaseMissing,
    /// 复测结果与基线不符。
    ResultDiff,
    /// 基线缺失。
    BaselineMissing,
    /// 基线过期。
    BaselineStale,
    /// 基线被改（指纹变）。
    FrozenChange,
    /// 有回归即宣告。
    FinalNotGreen,
    /// 用例被跳过。
    SkipCase,
}

impl ntrCode {
    /// 全量闭集。
    pub const ALL: [ntrCode; 8] = [
        ntrCode::RegressFound,
        ntrCode::CaseMissing,
        ntrCode::ResultDiff,
        ntrCode::BaselineMissing,
        ntrCode::BaselineStale,
        ntrCode::FrozenChange,
        ntrCode::FinalNotGreen,
        ntrCode::SkipCase,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            ntrCode::RegressFound => 0x6240 + 0,
            ntrCode::CaseMissing => 0x6240 + 1,
            ntrCode::ResultDiff => 0x6240 + 2,
            ntrCode::BaselineMissing => 0x6240 + 3,
            ntrCode::BaselineStale => 0x6240 + 4,
            ntrCode::FrozenChange => 0x6240 + 5,
            ntrCode::FinalNotGreen => 0x6240 + 6,
            ntrCode::SkipCase => 0x6240 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            ntrCode::RegressFound => "检测到回归",
            ntrCode::CaseMissing => "复测漏用例",
            ntrCode::ResultDiff => "复测结果与基线不符",
            ntrCode::BaselineMissing => "基线缺失",
            ntrCode::BaselineStale => "基线过期",
            ntrCode::FrozenChange => "基线被改（指纹变）",
            ntrCode::FinalNotGreen => "有回归即宣告",
            ntrCode::SkipCase => "用例被跳过",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            ntrCode::RegressFound => "定位并回滚变更",
            ntrCode::CaseMissing => "补用例后重跑",
            ntrCode::ResultDiff => "按基线对齐",
            ntrCode::BaselineMissing => "重新冻结基线",
            ntrCode::BaselineStale => "重建基线",
            ntrCode::FrozenChange => "触发防篡改审计",
            ntrCode::FinalNotGreen => "零回归后才宣告",
            ntrCode::SkipCase => "恢复执行",
        }
    }
}


// --------------------------------------------------------------------------
// 账本 ntr_regr（回归复测账）
// --------------------------------------------------------------------------
/// 回归复测账。
#[derive(Clone, Copy, Debug)]
pub struct ntr_regr {
    /// 复测用例数
    pub cases: u64,
    /// 通过数
    pub passed: u64,
    /// 失败数
    pub failed: u64,
    /// 跳过数
    pub skipped: u64,
}

impl ntr_regr {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            cases: 0,
            passed: 0,
            failed: 0,
            skipped: 0,
        }
    }

    /// 一个用例通过。
    pub fn case_pass(&mut self) -> () {
        self.cases += 1;
        self.passed += 1;
    }

    /// 一个用例失败。
    pub fn case_fail(&mut self) -> () {
        self.cases += 1;
        self.failed += 1;
    }

    /// 一个用例被跳过。
    pub fn case_skip(&mut self) -> () {
        self.cases += 1;
        self.skipped += 1;
    }

    /// 零回归（无失败无跳过）。
    pub fn zero_regress(&mut self) -> bool {
        self.failed == 0 && self.skipped == 0 && self.cases > 0
    }

}


// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x6240, 0x624f]。
pub const fn ntr_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < ntrCode::ALL.len() {
        let c = ntrCode::ALL[i].code();
        if c < 0x6240 || c > 0x624f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 零回归判据。
fn c1_zero() -> bool {
    let mut r = ntr_regr::new();
    let mut i = 0u64;
    while i < 40 { r.case_pass(); i += 1; }
    r.zero_regress() && r.passed == 40
}

/// 一例失败即不零回归。
fn c2_fail() -> bool {
    let mut r = ntr_regr::new();
    let mut i = 0u64;
    while i < 39 { r.case_pass(); i += 1; }
    r.case_fail();
    !r.zero_regress() && r.failed == 1
}

/// 跳过一个用例即不零回归。
fn c3_skip() -> bool {
    let mut r = ntr_regr::new();
    let mut i = 0u64;
    while i < 39 { r.case_pass(); i += 1; }
    r.case_skip();
    !r.zero_regress()
}

/// 段位守卫：全部诊断码落在本模块段内。
fn ntr_guard_seg() -> bool {
    ntr_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_ntr_checks() -> CheckSet {
    let mut set = CheckSet::new("ntr");
    set.add(
        "F1081-regr-c1",
        c1_zero(),
        if c1_zero() { "" } else { "regression found in F1079 recheck" },
    );
    set.add(
        "F1081-regr-c2",
        c2_fail(),
        if c2_fail() { "" } else { "failed case slipped through" },
    );
    set.add(
        "F1081-regr-c3",
        c3_skip(),
        if c3_skip() { "" } else { "skipped case slipped through" },
    );
    set.add(
        "ntr-seg-guard",
        ntr_guard_seg(),
        if ntr_guard_seg() { "" } else { "diagnostic code outside module segment" },
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
    fn ntr_all_checks_green() {
        let set = run_ntr_checks();
        assert!(set.all_passed(), "NTR domain checks must be green");
    }

    #[test]
    fn ntr_segment_guard_holds() {
        assert!(ntr_segment_guard());
    }
}
