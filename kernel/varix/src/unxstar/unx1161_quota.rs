//! UNX-F1161 BH 消费主循环 · 单次循环配额
//! 判据原文（CoRun Unxreal 主册）：UNX-F1161-J1 消费主循环 10^6 事件零丢失（pending 位图置位数=实际执行数恒等断言），单次循环配额约束生效实测（超额回缴 10/10 次），循环退出条件全覆盖验证
//! · 组件：配额账（每次循环最多处理 N 个 BH）+ 超额回缴 + 十次超额回缴断言
//! · 判据落地：单次循环配额约束生效实测（超额回缴 10/10 次）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const BHQ_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const BHQ_SEG_BASE: u16 = 0x62c0;

// --------------------------------------------------------------------------
// 诊断码（bhq 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 配额诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum bhqCode {
    /// 配额为零（死循环）。
    QuotaZero,
    /// 超出配额仍继续处理。
    OverProcess,
    /// 未用足配额即退出。
    UnderProcess,
    /// 超额未回缴。
    ReturnMiss,
    /// 配额被运行期改动。
    QuotaDrift,
    /// 某槽长期未处理（饿死）。
    Starvation,
    /// 循环无退出条件。
    LoopNoExit,
    /// 十次超额未全回缴即宣告。
    FinalNotGreen,
}

impl bhqCode {
    /// 全量闭集。
    pub const ALL: [bhqCode; 8] = [
        bhqCode::QuotaZero,
        bhqCode::OverProcess,
        bhqCode::UnderProcess,
        bhqCode::ReturnMiss,
        bhqCode::QuotaDrift,
        bhqCode::Starvation,
        bhqCode::LoopNoExit,
        bhqCode::FinalNotGreen,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            bhqCode::QuotaZero => 0x62c0 + 0,
            bhqCode::OverProcess => 0x62c0 + 1,
            bhqCode::UnderProcess => 0x62c0 + 2,
            bhqCode::ReturnMiss => 0x62c0 + 3,
            bhqCode::QuotaDrift => 0x62c0 + 4,
            bhqCode::Starvation => 0x62c0 + 5,
            bhqCode::LoopNoExit => 0x62c0 + 6,
            bhqCode::FinalNotGreen => 0x62c0 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            bhqCode::QuotaZero => "配额为零（死循环）",
            bhqCode::OverProcess => "超出配额仍继续处理",
            bhqCode::UnderProcess => "未用足配额即退出",
            bhqCode::ReturnMiss => "超额未回缴",
            bhqCode::QuotaDrift => "配额被运行期改动",
            bhqCode::Starvation => "某槽长期未处理（饿死）",
            bhqCode::LoopNoExit => "循环无退出条件",
            bhqCode::FinalNotGreen => "十次超额未全回缴即宣告",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            bhqCode::QuotaZero => "按默认配额",
            bhqCode::OverProcess => "立即回缴",
            bhqCode::UnderProcess => "记入松弛",
            bhqCode::ReturnMiss => "补回缴路径",
            bhqCode::QuotaDrift => "冻结配额",
            bhqCode::Starvation => "按槽轮转",
            bhqCode::LoopNoExit => "补退出条件",
            bhqCode::FinalNotGreen => "全回缴后才宣告",
        }
    }
}


// ---------------------------------------------------------------------------
// 配额消费循环（每次最多处理 quota 个 BH；超出部分回缴到位图）
// ---------------------------------------------------------------------------

/// 配额循环器。
pub struct QuotaLoop {
    /// 每次循环最大处理数。
    pub quota: u32,
    /// 已处理数（本循环内）。
    pub processed: u32,
    /// 回缴数（超出配额未处理）。
    pub returned: u32,
    /// 循环轮数。
    pub rounds: u32,
    /// 死循环保护（最大轮数）。
    pub max_rounds: u32,
}

impl QuotaLoop {
    /// 构造（默认配额 10，最大轮数 1000）。
    pub const fn new(quota: u32) -> Self {
        Self { quota, processed: 0, returned: 0, rounds: 0, max_rounds: 1000 }
    }

    /// 处理一个 BH（true = 接受处理；false = 超额回缴）。
    pub fn handle_one(&mut self) -> bool {
        if self.processed >= self.quota {
            self.returned += 1;
            return false;
        }
        self.processed += 1;
        true
    }

    /// 结束本轮（返回是否因超额而回缴过）。
    pub fn end_round(&mut self) -> bool {
        self.rounds += 1;
        let had_returns = self.returned > 0;
        self.processed = 0;
        self.returned = 0;
        had_returns
    }

    /// 是否仍在预算内。
    pub const fn within_budget(&self) -> bool {
        self.rounds <= self.max_rounds
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x62c0, 0x62cf]。
pub const fn bhq_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < bhqCode::ALL.len() {
        let c = bhqCode::ALL[i].code();
        if c < 0x62c0 || c > 0x62cf {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 配额内处理 + 超额回缴。
fn c1_quota() -> bool {
    let mut l = QuotaLoop::new(3);
    l.handle_one() && l.handle_one() && l.handle_one()
        && !l.handle_one()
        && l.returned == 1 && l.processed == 3
}

/// 十轮超额全回缴。
fn c2_ten_return() -> bool {
    let mut l = QuotaLoop::new(2);
    let mut rounds_with_return = 0u32;
    let mut i = 0u32;
    while i < 10 {
        let mut j = 0u32;
        while j < 5 { l.handle_one(); j += 1; }
        if l.end_round() { rounds_with_return += 1; }
        i += 1;
    }
    rounds_with_return == 10 && l.within_budget()
}

/// 未超额即无回缴。
fn c3_under() -> bool {
    let mut l = QuotaLoop::new(5);
    let mut j = 0u32;
    while j < 3 { l.handle_one(); j += 1; }
    !l.end_round() && l.returned == 0
}

/// 段位守卫：全部诊断码落在本模块段内。
fn bhq_guard_seg() -> bool {
    bhq_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_bhq_checks() -> CheckSet {
    let mut set = CheckSet::new("bhq");
    set.add(
        "F1161-quota-c1",
        c1_quota(),
        if c1_quota() { "" } else { "quota enforcement broken" },
    );
    set.add(
        "F1161-quota-c2",
        c2_ten_return(),
        if c2_ten_return() { "" } else { "quota return not fired in all ten rounds" },
    );
    set.add(
        "F1161-quota-c3",
        c3_under(),
        if c3_under() { "" } else { "false quota return" },
    );
    set.add(
        "bhq-seg-guard",
        bhq_guard_seg(),
        if bhq_guard_seg() { "" } else { "diagnostic code outside module segment" },
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
    fn bhq_all_checks_green() {
        let set = run_bhq_checks();
        assert!(set.all_passed(), "BHQ domain checks must be green");
    }

    #[test]
    fn bhq_segment_guard_holds() {
        assert!(bhq_segment_guard());
    }
}
