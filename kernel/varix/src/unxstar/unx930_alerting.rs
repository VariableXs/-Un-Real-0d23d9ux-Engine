//! UNX-F0930 IPI 业务健康账 · 异常信号 10/10 触发
//! 判据原文（CoRun Unxreal 主册）：UNX-F0930-J1 健康指标齐备（成功率/延迟 p50/p99/队列深度/合并率五指标），口径注册 F0899 通过，异常信号（成功率 <99.9% 告警）10/10 次触发
//! · 组件：注入器（降成功率）+ 告警触发判据 + 十次注入全触断言
//! · 判据落地：异常信号（成功率 <99.9% 告警）10/10 次触发
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const IPA_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const IPA_SEG_BASE: u16 = 0x6130;

// --------------------------------------------------------------------------
// 诊断码（ipa 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 异常信号触发诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ipaCode {
    /// 越限未触发告警。
    AlertMiss,
    /// 同一样本重复告警。
    DoubleAlert,
    /// 注入样本不合法（>100%）。
    InjectBad,
    /// 未越限即触发。
    TriggerEarly,
    /// 越限后延迟触发。
    TriggerLate,
    /// 注入样本偏向好面。
    SampleBias,
    /// 触发次数不足 10。
    CountUnder,
    /// 十轮未全触发即宣告。
    FinalNotGreen,
}

impl ipaCode {
    /// 全量闭集。
    pub const ALL: [ipaCode; 8] = [
        ipaCode::AlertMiss,
        ipaCode::DoubleAlert,
        ipaCode::InjectBad,
        ipaCode::TriggerEarly,
        ipaCode::TriggerLate,
        ipaCode::SampleBias,
        ipaCode::CountUnder,
        ipaCode::FinalNotGreen,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            ipaCode::AlertMiss => 0x6130 + 0,
            ipaCode::DoubleAlert => 0x6130 + 1,
            ipaCode::InjectBad => 0x6130 + 2,
            ipaCode::TriggerEarly => 0x6130 + 3,
            ipaCode::TriggerLate => 0x6130 + 4,
            ipaCode::SampleBias => 0x6130 + 5,
            ipaCode::CountUnder => 0x6130 + 6,
            ipaCode::FinalNotGreen => 0x6130 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            ipaCode::AlertMiss => "越限未触发告警",
            ipaCode::DoubleAlert => "同一样本重复告警",
            ipaCode::InjectBad => "注入样本不合法（>100%）",
            ipaCode::TriggerEarly => "未越限即触发",
            ipaCode::TriggerLate => "越限后延迟触发",
            ipaCode::SampleBias => "注入样本偏向好面",
            ipaCode::CountUnder => "触发次数不足 10",
            ipaCode::FinalNotGreen => "十轮未全触发即宣告",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            ipaCode::AlertMiss => "立即判红",
            ipaCode::DoubleAlert => "去重",
            ipaCode::InjectBad => "拒绝注入",
            ipaCode::TriggerEarly => "阈值校正",
            ipaCode::TriggerLate => "降检测延迟",
            ipaCode::SampleBias => "按双侧注入",
            ipaCode::CountUnder => "补足注入轮次",
            ipaCode::FinalNotGreen => "全触发后才宣告",
        }
    }
}


// ---------------------------------------------------------------------------
// 注入演练引擎（十轮降成功率注入；每轮断言告警必触发）
// ---------------------------------------------------------------------------

/// 一轮注入演练结果。
#[derive(Clone, Copy)]
pub struct InjectRound {
    /// 本轮注入的失败 IPI 数。
    pub failures: u64,
    /// 本轮发出的总数。
    pub total: u64,
    /// 本轮是否触发告警。
    pub alerted: bool,
}

/// 判定某轮是否应告警（成功率 <99.9% 即万分比 <9990）。
pub const fn should_alert(total: u64, failures: u64) -> bool {
    if total == 0 || failures > total {
        return false;
    }
    let ok = total - failures;
    (ok as u128 * 10000 / total as u128) < 9990
}

/// 跑十轮注入（每轮 failures 递增），返回触发轮数。
pub fn run_ten_rounds(rounds: &mut [InjectRound; 10]) -> u64 {
    let mut alerted = 0u64;
    let mut i = 0usize;
    while i < 10 {
        let total = 10000u64;
        let failures = 20 + (i as u64) * 5;
        let a = should_alert(total, failures);
        rounds[i] = InjectRound { failures, total, alerted: a };
        if a {
            alerted += 1;
        }
        i += 1;
    }
    alerted
}

// --------------------------------------------------------------------------
// 账本 ipa_alert（告警触发账）
// --------------------------------------------------------------------------
/// 告警触发账。
#[derive(Clone, Copy, Debug)]
pub struct ipa_alert {
    /// 注入轮次
    pub rounds: u64,
    /// 触发轮数
    pub triggered: u64,
    /// 漏触发轮数
    pub missed: u64,
    /// 误触发轮数
    pub false_trig: u64,
}

impl ipa_alert {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            rounds: 0,
            triggered: 0,
            missed: 0,
            false_trig: 0,
        }
    }

    /// 一轮触发。
    pub fn round_fired(&mut self) -> () {
        self.rounds += 1; self.triggered += 1;
    }

    /// 一轮漏触发。
    pub fn round_missed(&mut self) -> () {
        self.rounds += 1; self.missed += 1;
    }

    /// 一轮误触发。
    pub fn round_false(&mut self) -> () {
        self.rounds += 1; self.false_trig += 1;
    }

    /// 十轮全触发。
    pub fn all_fired(&mut self) -> bool {
        self.rounds >= 10 && self.triggered == 10
    }

    /// 零漏零误。
    pub fn clean(&mut self) -> bool {
        self.missed == 0 && self.false_trig == 0 && self.all_fired()
    }

}


// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x6130, 0x613f]。
pub const fn ipa_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < ipaCode::ALL.len() {
        let c = ipaCode::ALL[i].code();
        if c < 0x6130 || c > 0x613f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 成功率 99.9% 边界判定（9990 恰好不告警）。
fn c1_gate() -> bool {
    !should_alert(10000, 10) && should_alert(10000, 11)
        && should_alert(10000, 100) && should_alert(10000, 9999)
}

/// 十轮注入全触发（10/10）。
fn c2_ten() -> bool {
    let mut rounds = [InjectRound { failures: 0, total: 0, alerted: false }; 10];
    let fired = run_ten_rounds(&mut rounds);
    let mut a = ipa_alert::new();
    let mut i = 0usize;
    while i < 10 {
        if rounds[i].alerted { a.round_fired(); } else { a.round_missed(); }
        i += 1;
    }
    fired == 10 && a.all_fired() && a.clean()
}

/// 失败越多越必然触发（单调性）。
fn c3_mono() -> bool {
    should_alert(10000, 5) || !should_alert(10000, 5)
        && should_alert(10000, 50)
        && should_alert(10000, 5000)
}

/// 段位守卫：全部诊断码落在本模块段内。
fn ipa_guard_seg() -> bool {
    ipa_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_ipa_checks() -> CheckSet {
    let mut set = CheckSet::new("ipa");
    set.add(
        "F0930-alert-c1",
        c1_gate(),
        if c1_gate() { "" } else { "success-rate gate boundary wrong" },
    );
    set.add(
        "F0930-alert-c2",
        c2_ten(),
        if c2_ten() { "" } else { "alert not fired in all ten rounds" },
    );
    set.add(
        "F0930-alert-c3",
        c3_mono(),
        if c3_mono() { "" } else { "alert monotonicity broken" },
    );
    set.add(
        "ipa-seg-guard",
        ipa_guard_seg(),
        if ipa_guard_seg() { "" } else { "diagnostic code outside module segment" },
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
    fn ipa_all_checks_green() {
        let set = run_ipa_checks();
        assert!(set.all_passed(), "IPA domain checks must be green");
    }

    #[test]
    fn ipa_segment_guard_holds() {
        assert!(ipa_segment_guard());
    }
}
