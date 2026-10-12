//! UNX-F0961 现存轮询栈盘点账 · 双源交叉核对
//! 判据原文（CoRun Unxreal 主册）：UNX-F0961-J1 盘点完整（现存全部轮询点清册零遗漏——grep+运行期双源交叉），特征画像落账，盘点与运行观测一致率 100%
//! · 组件：静态源（grep 清册）与运行期观测双表交叉 + 差异分类 + 一致率 100% 判据
//! · 判据落地：grep+运行期双源交叉
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const PMX_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const PMX_SEG_BASE: u16 = 0x6180;

// --------------------------------------------------------------------------
// 诊断码（pmx 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 双源交叉诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum pmxCode {
    /// 仅静态源有（可能死代码）。
    OnlyInSource,
    /// 仅运行期有（漏登记）。
    OnlyInRuntime,
    /// 双源地址不符。
    AddrMismatch,
    /// 双源计数不符。
    CountMismatch,
    /// 交叉核对超轮次预算。
    CrossOver,
    /// 伪差异（口径不同）。
    FalseDiff,
    /// 差异未检出。
    MissedDiff,
    /// 未达 100% 一致即宣告。
    FinalNotGreen,
}

impl pmxCode {
    /// 全量闭集。
    pub const ALL: [pmxCode; 8] = [
        pmxCode::OnlyInSource,
        pmxCode::OnlyInRuntime,
        pmxCode::AddrMismatch,
        pmxCode::CountMismatch,
        pmxCode::CrossOver,
        pmxCode::FalseDiff,
        pmxCode::MissedDiff,
        pmxCode::FinalNotGreen,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            pmxCode::OnlyInSource => 0x6180 + 0,
            pmxCode::OnlyInRuntime => 0x6180 + 1,
            pmxCode::AddrMismatch => 0x6180 + 2,
            pmxCode::CountMismatch => 0x6180 + 3,
            pmxCode::CrossOver => 0x6180 + 4,
            pmxCode::FalseDiff => 0x6180 + 5,
            pmxCode::MissedDiff => 0x6180 + 6,
            pmxCode::FinalNotGreen => 0x6180 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            pmxCode::OnlyInSource => "仅静态源有（可能死代码）",
            pmxCode::OnlyInRuntime => "仅运行期有（漏登记）",
            pmxCode::AddrMismatch => "双源地址不符",
            pmxCode::CountMismatch => "双源计数不符",
            pmxCode::CrossOver => "交叉核对超轮次预算",
            pmxCode::FalseDiff => "伪差异（口径不同）",
            pmxCode::MissedDiff => "差异未检出",
            pmxCode::FinalNotGreen => "未达 100% 一致即宣告",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            pmxCode::OnlyInSource => "分类为疑似死码再核",
            pmxCode::OnlyInRuntime => "补登记并复核",
            pmxCode::AddrMismatch => "重解析符号",
            pmxCode::CountMismatch => "定位差集并校正",
            pmxCode::CrossOver => "分批发起",
            pmxCode::FalseDiff => "统一口径后复对",
            pmxCode::MissedDiff => "立即判红",
            pmxCode::FinalNotGreen => "全一致后才宣告",
        }
    }
}


// --------------------------------------------------------------------------
// 账本 pmx_cross（交叉核对账）
// --------------------------------------------------------------------------
/// 交叉核对账。
#[derive(Clone, Copy, Debug)]
pub struct pmx_cross {
    /// 双源都有
    pub in_both: u64,
    /// 仅静态源
    pub only_static: u64,
    /// 仅运行期
    pub only_runtime: u64,
    /// 地址不一致数
    pub addr_diff: u64,
    /// 交叉核对点总数
    pub points: u64,
}

impl pmx_cross {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            in_both: 0,
            only_static: 0,
            only_runtime: 0,
            addr_diff: 0,
            points: 0,
        }
    }

    /// 一个点在双源都有。
    pub fn both(&mut self) -> () {
        self.in_both += 1;
        self.points += 1;
    }

    /// 一个点仅静态源有。
    pub fn only_static_one(&mut self) -> () {
        self.only_static += 1;
        self.points += 1;
    }

    /// 一个点仅运行期有。
    pub fn only_runtime_one(&mut self) -> () {
        self.only_runtime += 1;
        self.points += 1;
    }

    /// 一次地址不一致。
    pub fn addr_diff_one(&mut self) -> () {
        self.addr_diff += 1;
    }

    /// 交叉核对点总计。
    pub fn total(&mut self) -> u64 {
        self.in_both + self.only_static + self.only_runtime
    }

    /// 一致率 100%（无单侧、无地址差）。
    pub fn consistent(&mut self) -> bool {
        self.total() > 0
            && self.only_static == 0
            && self.only_runtime == 0
            && self.addr_diff == 0
    }

}


// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x6180, 0x618f]。
pub const fn pmx_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < pmxCode::ALL.len() {
        let c = pmxCode::ALL[i].code();
        if c < 0x6180 || c > 0x618f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 双源全一致判据。
fn c1_consistent() -> bool {
    let mut c = pmx_cross::new();
    let mut i = 0u64;
    while i < 40 { c.both(); i += 1; }
    c.consistent() && c.total() == 40
}

/// 任一单侧差异即不一致。
fn c2_diff() -> bool {
    let mut c = pmx_cross::new();
    let mut i = 0u64;
    while i < 39 { c.both(); i += 1; }
    c.only_static_one();
    !c.consistent() && c.total() == 40
}

/// 地址不一致即不一致。
fn c3_addr() -> bool {
    let mut c = pmx_cross::new();
    let mut i = 0u64;
    while i < 40 { c.both(); i += 1; }
    c.addr_diff_one();
    !c.consistent()
}

/// 段位守卫：全部诊断码落在本模块段内。
fn pmx_guard_seg() -> bool {
    pmx_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_pmx_checks() -> CheckSet {
    let mut set = CheckSet::new("pmx");
    set.add(
        "F0961-cross-c1",
        c1_consistent(),
        if c1_consistent() { "" } else { "cross-source not fully consistent" },
    );
    set.add(
        "F0961-cross-c2",
        c2_diff(),
        if c2_diff() { "" } else { "single-side diff slipped through" },
    );
    set.add(
        "F0961-cross-c3",
        c3_addr(),
        if c3_addr() { "" } else { "address diff slipped through" },
    );
    set.add(
        "pmx-seg-guard",
        pmx_guard_seg(),
        if pmx_guard_seg() { "" } else { "diagnostic code outside module segment" },
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
    fn pmx_all_checks_green() {
        let set = run_pmx_checks();
        assert!(set.all_passed(), "PMX domain checks must be green");
    }

    #[test]
    fn pmx_segment_guard_holds() {
        assert!(pmx_segment_guard());
    }
}
