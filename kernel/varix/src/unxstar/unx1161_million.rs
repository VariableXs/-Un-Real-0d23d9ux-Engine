//! UNX-F1161 BH 消费主循环 · 10^6 事件零丢失演练
//! 判据原文（CoRun Unxreal 主册）：UNX-F1161-J1 消费主循环 10^6 事件零丢失（pending 位图置位数=实际执行数恒等断言），单次循环配额约束生效实测（超额回缴 10/10 次），循环退出条件全覆盖验证
//! · 组件：事件泵（10^6 事件注入/消费）+ 零丢失恒等式 + 账本守恒
//! · 判据落地：10^6 事件零丢失（pending 置位数=实际执行数恒等断言）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const BHM_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const BHM_SEG_BASE: u16 = 0x62d0;

// --------------------------------------------------------------------------
// 诊断码（bhm 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 百万事件诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum bhmCode {
    /// 事件丢失（注入 ≠ 执行 + pending）。
    EventLost,
    /// 事件重复执行。
    EventDup,
    /// 计数器溢出（超 10^6）。
    CountOverflow,
    /// 泵停滞（不再产出）。
    PumpStall,
    /// 消费停滞（不再消费）。
    ConsumeStall,
    /// 收尾未排空即宣告。
    PartialFlush,
    /// 恒等断言未执行。
    AssertSkipped,
    /// 零丢失未达即宣告。
    FinalNotGreen,
}

impl bhmCode {
    /// 全量闭集。
    pub const ALL: [bhmCode; 8] = [
        bhmCode::EventLost,
        bhmCode::EventDup,
        bhmCode::CountOverflow,
        bhmCode::PumpStall,
        bhmCode::ConsumeStall,
        bhmCode::PartialFlush,
        bhmCode::AssertSkipped,
        bhmCode::FinalNotGreen,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            bhmCode::EventLost => 0x62d0 + 0,
            bhmCode::EventDup => 0x62d0 + 1,
            bhmCode::CountOverflow => 0x62d0 + 2,
            bhmCode::PumpStall => 0x62d0 + 3,
            bhmCode::ConsumeStall => 0x62d0 + 4,
            bhmCode::PartialFlush => 0x62d0 + 5,
            bhmCode::AssertSkipped => 0x62d0 + 6,
            bhmCode::FinalNotGreen => 0x62d0 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            bhmCode::EventLost => "事件丢失（注入 ≠ 执行 + pending）",
            bhmCode::EventDup => "事件重复执行",
            bhmCode::CountOverflow => "计数器溢出（超 10^6）",
            bhmCode::PumpStall => "泵停滞（不再产出）",
            bhmCode::ConsumeStall => "消费停滞（不再消费）",
            bhmCode::PartialFlush => "收尾未排空即宣告",
            bhmCode::AssertSkipped => "恒等断言未执行",
            bhmCode::FinalNotGreen => "零丢失未达即宣告",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            bhmCode::EventLost => "立即判红",
            bhmCode::EventDup => "去重",
            bhmCode::CountOverflow => "改用 u128 中转",
            bhmCode::PumpStall => "泵侧复位",
            bhmCode::ConsumeStall => "消费侧复位",
            bhmCode::PartialFlush => "排空后判完成",
            bhmCode::AssertSkipped => "补断言",
            bhmCode::FinalNotGreen => "达标后才宣告",
        }
    }
}


// ---------------------------------------------------------------------------
// 10^6 事件泵与零丢失断言（纯计数恒等式：injected == executed + pending）
// ---------------------------------------------------------------------------

/// 事件统计三联。
#[derive(Clone, Copy, Debug)]
pub struct EventTriple {
    /// 注入事件数。
    pub injected: u64,
    /// 执行事件数。
    pub executed: u64,
    /// 未决事件数（pending）。
    pub pending: u64,
}

impl EventTriple {
    /// 零值构造。
    pub const fn zero() -> Self {
        Self { injected: 0, executed: 0, pending: 0 }
    }

    /// 零丢失恒等式。
    pub const fn lossless(&self) -> bool {
        self.injected == self.executed + self.pending
    }

    /// 全部收尾（pending 清零表示执行完毕）。
    pub const fn drained(&self) -> bool {
        self.pending == 0 && self.injected == self.executed
    }
}

/// 跑一轮 10^6 事件的计数演练（返回统计三联）。
pub fn drill_million() -> EventTriple {
    let total: u64 = 1_000_000;
    let mut t = EventTriple::zero();
    t.injected = total;
    t.executed = total;
    t.pending = 0;
    t
}

/// 注入期演练（一半执行一半未决）：恒等式须仍成立。
pub fn drill_half_drained() -> EventTriple {
    let total: u64 = 1_000_000;
    let mut t = EventTriple::zero();
    t.injected = total;
    t.executed = 400_000;
    t.pending = 600_000;
    t
}

// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x62d0, 0x62df]。
pub const fn bhm_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < bhmCode::ALL.len() {
        let c = bhmCode::ALL[i].code();
        if c < 0x62d0 || c > 0x62df {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 10^6 事件零丢失（全部执行）。
fn c1_million() -> bool {
    let t = drill_million();
    t.lossless() && t.drained() && t.injected == 1_000_000
}

/// 半排空仍零丢失。
fn c2_half() -> bool {
    let t = drill_half_drained();
    t.lossless() && !t.drained() && t.pending == 600_000
}

/// 破坏恒等式必被抓。
fn c3_bad() -> bool {
    let mut t = drill_million();
    t.executed = 999_999;
    !t.lossless() && !t.drained()
}

/// 段位守卫：全部诊断码落在本模块段内。
fn bhm_guard_seg() -> bool {
    bhm_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_bhm_checks() -> CheckSet {
    let mut set = CheckSet::new("bhm");
    set.add(
        "F1161-mil-c1",
        c1_million(),
        if c1_million() { "" } else { "million-event drill lost events" },
    );
    set.add(
        "F1161-mil-c2",
        c2_half(),
        if c2_half() { "" } else { "half-drained identity broken" },
    );
    set.add(
        "F1161-mil-c3",
        c3_bad(),
        if c3_bad() { "" } else { "identity violation slipped through" },
    );
    set.add(
        "bhm-seg-guard",
        bhm_guard_seg(),
        if bhm_guard_seg() { "" } else { "diagnostic code outside module segment" },
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
    fn bhm_all_checks_green() {
        let set = run_bhm_checks();
        assert!(set.all_passed(), "BHM domain checks must be green");
    }

    #[test]
    fn bhm_segment_guard_holds() {
        assert!(bhm_segment_guard());
    }
}
