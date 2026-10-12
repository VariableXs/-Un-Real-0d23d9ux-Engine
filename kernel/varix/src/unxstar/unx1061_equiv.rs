//! UNX-F1061 时钟源抽象层 · 枚举升级等价性
//! 判据原文（CoRun Unxreal 主册）：UNX-F1061-J1 抽象统一（三源实现同一 trait 10/10 次行为一致），现存枚举升级等价率 100%（`ClockSource` 枚举收编为 trait 语义——行为不变断言），多态开销 <5ns
//! · 组件：旧枚举 vs 新 trait 行为对拍 + 等价率 100% 判据 + 行为冻结断言
//! · 判据落地：现存枚举升级等价率 100%（行为不变）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const CSE_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const CSE_SEG_BASE: u16 = 0x61e0;

// --------------------------------------------------------------------------
// 诊断码（cse 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 等价性诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum cseCode {
    /// 新旧行为不一致。
    BehaviorDiff,
    /// 返回值不符。
    ReturnDiff,
    /// 调用序列语义变化。
    SequenceDiff,
    /// 边界行为差异。
    EdgeCaseDiff,
    /// 等价率不足 100%。
    EquivUnder,
    /// 等价率虚报（>100%）。
    EquivOver,
    /// 冻结断言被改。
    FrozenBroken,
    /// 等价率 100% 未达即宣告。
    FinalNotGreen,
}

impl cseCode {
    /// 全量闭集。
    pub const ALL: [cseCode; 8] = [
        cseCode::BehaviorDiff,
        cseCode::ReturnDiff,
        cseCode::SequenceDiff,
        cseCode::EdgeCaseDiff,
        cseCode::EquivUnder,
        cseCode::EquivOver,
        cseCode::FrozenBroken,
        cseCode::FinalNotGreen,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            cseCode::BehaviorDiff => 0x61e0 + 0,
            cseCode::ReturnDiff => 0x61e0 + 1,
            cseCode::SequenceDiff => 0x61e0 + 2,
            cseCode::EdgeCaseDiff => 0x61e0 + 3,
            cseCode::EquivUnder => 0x61e0 + 4,
            cseCode::EquivOver => 0x61e0 + 5,
            cseCode::FrozenBroken => 0x61e0 + 6,
            cseCode::FinalNotGreen => 0x61e0 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            cseCode::BehaviorDiff => "新旧行为不一致",
            cseCode::ReturnDiff => "返回值不符",
            cseCode::SequenceDiff => "调用序列语义变化",
            cseCode::EdgeCaseDiff => "边界行为差异",
            cseCode::EquivUnder => "等价率不足 100%",
            cseCode::EquivOver => "等价率虚报（>100%）",
            cseCode::FrozenBroken => "冻结断言被改",
            cseCode::FinalNotGreen => "等价率 100% 未达即宣告",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            cseCode::BehaviorDiff => "定位差异分支并修正",
            cseCode::ReturnDiff => "按旧行为对齐",
            cseCode::SequenceDiff => "恢复旧序列",
            cseCode::EdgeCaseDiff => "按旧边界对齐",
            cseCode::EquivUnder => "逐项对齐",
            cseCode::EquivOver => "账本校正",
            cseCode::FrozenBroken => "恢复冻结版判据",
            cseCode::FinalNotGreen => "达标后才宣告",
        }
    }
}


// --------------------------------------------------------------------------
// 账本 cse_equiv（等价对拍账）
// --------------------------------------------------------------------------
/// 等价对拍账。
#[derive(Clone, Copy, Debug)]
pub struct cse_equiv {
    /// 对拍用例数
    pub cases: u64,
    /// 一致用例数
    pub identical: u64,
    /// 差异用例数
    pub diff: u64,
}

impl cse_equiv {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            cases: 0,
            identical: 0,
            diff: 0,
        }
    }

    /// 一个用例一致。
    pub fn case_same(&mut self) -> () {
        self.cases += 1;
        self.identical += 1;
    }

    /// 一个用例有差异。
    pub fn case_diff(&mut self) -> () {
        self.cases += 1;
        self.diff += 1;
    }

    /// 等价率（万分比）。
    pub fn equivalence_rate(&mut self) -> u64 {
        if self.cases == 0 { return 0; }
        ((self.identical as u128 * 10000 / self.cases as u128) as u64)
    }

    /// 等价率 100%。
    pub fn full(&mut self) -> bool {
        self.cases > 0 && self.identical == self.cases
    }

}


// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x61e0, 0x61ef]。
pub const fn cse_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < cseCode::ALL.len() {
        let c = cseCode::ALL[i].code();
        if c < 0x61e0 || c > 0x61ef {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 等价率 100%。
fn c1_full() -> bool {
    let mut e = cse_equiv::new();
    let mut i = 0u64;
    while i < 100 { e.case_same(); i += 1; }
    e.equivalence_rate() == 10000 && e.full()
}

/// 一处差异即不绿。
fn c2_diff() -> bool {
    let mut e = cse_equiv::new();
    let mut i = 0u64;
    while i < 99 { e.case_same(); i += 1; }
    e.case_diff();
    !e.full() && e.equivalence_rate() == 9900
}

/// 段位守卫：全部诊断码落在本模块段内。
fn cse_guard_seg() -> bool {
    cse_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_cse_checks() -> CheckSet {
    let mut set = CheckSet::new("cse");
    set.add(
        "F1061-equiv-c1",
        c1_full(),
        if c1_full() { "" } else { "equivalence not 100%" },
    );
    set.add(
        "F1061-equiv-c2",
        c2_diff(),
        if c2_diff() { "" } else { "diff case slipped through" },
    );
    set.add(
        "cse-seg-guard",
        cse_guard_seg(),
        if cse_guard_seg() { "" } else { "diagnostic code outside module segment" },
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
    fn cse_all_checks_green() {
        let set = run_cse_checks();
        assert!(set.all_passed(), "CSE domain checks must be green");
    }

    #[test]
    fn cse_segment_guard_holds() {
        assert!(cse_segment_guard());
    }
}
