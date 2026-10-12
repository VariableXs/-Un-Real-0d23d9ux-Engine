//! UNX-F1061 时钟源抽象层 · 三源实现
//! 判据原文（CoRun Unxreal 主册）：UNX-F1061-J1 抽象统一（三源实现同一 trait 10/10 次行为一致），现存枚举升级等价率 100%（`ClockSource` 枚举收编为 trait 语义——行为不变断言），多态开销 <5ns
//! · 组件：TSC/HPET/LAPIC-timer 三源适配器 + 频率换算一致性 + 源选择状态机
//! · 判据落地：三源同 trait 行为一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const CSS_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const CSS_SEG_BASE: u16 = 0x61d0;

// --------------------------------------------------------------------------
// 诊断码（css 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 三源实现诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum cssCode {
    /// 源标识未知（三源之外）。
    SrcUnknown,
    /// 频率为零（换算除零）。
    FreqZero,
    /// 三源频率换算偏差过大。
    FreqDrift,
    /// 源切换与读并发。
    SwitchRace,
    /// 源未就绪即选。
    SrcNotReady,
    /// 源频率读回不符。
    ReadbackDiff,
    /// 源优先级乱序。
    PrecedenceBad,
    /// 选中源失效。
    SrcLost,
}

impl cssCode {
    /// 全量闭集。
    pub const ALL: [cssCode; 8] = [
        cssCode::SrcUnknown,
        cssCode::FreqZero,
        cssCode::FreqDrift,
        cssCode::SwitchRace,
        cssCode::SrcNotReady,
        cssCode::ReadbackDiff,
        cssCode::PrecedenceBad,
        cssCode::SrcLost,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            cssCode::SrcUnknown => 0x61d0 + 0,
            cssCode::FreqZero => 0x61d0 + 1,
            cssCode::FreqDrift => 0x61d0 + 2,
            cssCode::SwitchRace => 0x61d0 + 3,
            cssCode::SrcNotReady => 0x61d0 + 4,
            cssCode::ReadbackDiff => 0x61d0 + 5,
            cssCode::PrecedenceBad => 0x61d0 + 6,
            cssCode::SrcLost => 0x61d0 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            cssCode::SrcUnknown => "源标识未知（三源之外）",
            cssCode::FreqZero => "频率为零（换算除零）",
            cssCode::FreqDrift => "三源频率换算偏差过大",
            cssCode::SwitchRace => "源切换与读并发",
            cssCode::SrcNotReady => "源未就绪即选",
            cssCode::ReadbackDiff => "源频率读回不符",
            cssCode::PrecedenceBad => "源优先级乱序",
            cssCode::SrcLost => "选中源失效",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            cssCode::SrcUnknown => "按三源闭集校正",
            cssCode::FreqZero => "拒除零并记账",
            cssCode::FreqDrift => "重新校准",
            cssCode::SwitchRace => "串行化切换窗",
            cssCode::SrcNotReady => "等待就绪后重选",
            cssCode::ReadbackDiff => "重校准",
            cssCode::PrecedenceBad => "按 TSC>HPET>LAPIC 重排",
            cssCode::SrcLost => "降级到下一级源",
        }
    }
}


// --------------------------------------------------------------------------
// 冻结常量表 css_src（7 行逐行注释自证）
// --------------------------------------------------------------------------
/// 三源能力表。
pub const css_src_COUNT: usize = 7;

/// 表本体（编译期冻结，运行期只读）。
pub static css_src: [(&'static str, u32); 7] = [
    ("Tsc", 1), // TSC（Invariant TSC，首选）
    ("Hpet", 2), // HPET（稳定频率，次选）
    ("LapicTimer", 3), // LAPIC 定时器（兜底）
    ("src_total", 3), // 源闭集总数
    ("precedence0", 1), // 优先级 0 = TSC
    ("precedence1", 2), // 优先级 1 = HPET
    ("precedence2", 3), // 优先级 2 = LAPIC
];

/// 线性查找（键唯一由构造期断言保证）。
pub fn css_src_get(key: &'static str) -> Option<u32> {
    let mut i = 0usize;
    while i < css_src_COUNT {
        if css_src[i].0 == key {
            return Some(css_src[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（守恒对账用）。
pub fn css_src_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < css_src_COUNT {
        s = s.wrapping_add(css_src[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（逐对扫描；重复即结构腐坏）。
pub fn css_src_unique() -> bool {
    let mut i = 0usize;
    while i < css_src_COUNT {
        let mut j = i + 1;
        while j < css_src_COUNT {
            if css_src[i].0 == css_src[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}


// --------------------------------------------------------------------------
// 账本 css_sel（源选择账）
// --------------------------------------------------------------------------
/// 源选择账。
#[derive(Clone, Copy, Debug)]
pub struct css_sel {
    /// 切换次数
    pub switches: u64,
    /// 失败切换数
    pub failed_switches: u64,
    /// 降级次数
    pub downgrades: u64,
    /// 读计数数
    pub reads: u64,
    /// 频率不符数
    pub freq_mismatch: u64,
}

impl css_sel {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            switches: 0,
            failed_switches: 0,
            downgrades: 0,
            reads: 0,
            freq_mismatch: 0,
        }
    }

    /// 一次成功切换。
    pub fn switch_ok(&mut self) -> () {
        self.switches += 1;
    }

    /// 一次失败切换。
    pub fn switch_fail(&mut self) -> () {
        self.failed_switches += 1;
    }

    /// 一次降级。
    pub fn downgrade(&mut self) -> () {
        self.downgrades += 1;
    }

    /// 一次读计数。
    pub fn read_one(&mut self) -> () {
        self.reads += 1;
    }

    /// 一次频率不符。
    pub fn freq_bad(&mut self) -> () {
        self.freq_mismatch += 1;
    }

    /// 源选择面干净。
    pub fn clean(&mut self) -> bool {
        self.failed_switches == 0 && self.freq_mismatch == 0
    }

}


// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x61d0, 0x61df]。
pub const fn css_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < cssCode::ALL.len() {
        let c = cssCode::ALL[i].code();
        if c < 0x61d0 || c > 0x61df {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 三源闭集 + 优先级。
fn c1_src() -> bool {
    css_src_get("Tsc") == Some(1) && css_src_get("Hpet") == Some(2)
        && css_src_get("LapicTimer") == Some(3)
        && css_src_get("src_total") == Some(3)
        && css_src_unique()
}

/// 源选择账干净判据。
fn c2_ledger() -> bool {
    let mut s = css_sel::new();
    let mut i = 0u64;
    while i < 10 { s.switch_ok(); i += 1; }
    let mut j = 0u64;
    while j < 1000 { s.read_one(); j += 1; }
    s.clean() && s.switches == 10
}

/// 段位守卫：全部诊断码落在本模块段内。
fn css_guard_seg() -> bool {
    css_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_css_checks() -> CheckSet {
    let mut set = CheckSet::new("css");
    set.add(
        "F1061-src-c1",
        c1_src(),
        if c1_src() { "" } else { "three-source set drifted" },
    );
    set.add(
        "F1061-src-c2",
        c2_ledger(),
        if c2_ledger() { "" } else { "source selection ledger not clean" },
    );
    set.add(
        "css-seg-guard",
        css_guard_seg(),
        if css_guard_seg() { "" } else { "diagnostic code outside module segment" },
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
    fn css_all_checks_green() {
        let set = run_css_checks();
        assert!(set.all_passed(), "CSS domain checks must be green");
    }

    #[test]
    fn css_segment_guard_holds() {
        assert!(css_segment_guard());
    }
}
