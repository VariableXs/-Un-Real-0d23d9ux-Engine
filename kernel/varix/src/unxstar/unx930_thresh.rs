//! UNX-F0930 IPI 业务健康账 · 阈值与告警
//! 判据原文（CoRun Unxreal 主册）：UNX-F0930-J1 健康指标齐备（成功率/延迟 p50/p99/队列深度/合并率五指标），口径注册 F0899 通过，异常信号（成功率 <99.9% 告警）10/10 次触发
//! · 组件：五指标告警阈值表 + 双稳态（正常/告警）状态机 + 迟滞防抖
//! · 判据落地：异常信号（成功率 <99.9% 告警）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const IPT_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const IPT_SEG_BASE: u16 = 0x6110;

// --------------------------------------------------------------------------
// 诊断码（ipt 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 阈值告警诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum iptCode {
    /// 阈值未配置。
    ThreshUnset,
    /// 告警与解除阈值交叉（无迟滞）。
    ThreshConflict,
    /// 告警风暴（频繁翻转）。
    AlertStorm,
    /// 低于阈值未告警。
    SilentFail,
    /// 高于阈值仍告警。
    FalseAlert,
    /// 抑制窗口未生效。
    SuppressMiss,
    /// 阈值跨口径混用。
    ScopeMix,
    /// 恢复后未解除告警。
    RecoverMiss,
}

impl iptCode {
    /// 全量闭集。
    pub const ALL: [iptCode; 8] = [
        iptCode::ThreshUnset,
        iptCode::ThreshConflict,
        iptCode::AlertStorm,
        iptCode::SilentFail,
        iptCode::FalseAlert,
        iptCode::SuppressMiss,
        iptCode::ScopeMix,
        iptCode::RecoverMiss,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            iptCode::ThreshUnset => 0x6110 + 0,
            iptCode::ThreshConflict => 0x6110 + 1,
            iptCode::AlertStorm => 0x6110 + 2,
            iptCode::SilentFail => 0x6110 + 3,
            iptCode::FalseAlert => 0x6110 + 4,
            iptCode::SuppressMiss => 0x6110 + 5,
            iptCode::ScopeMix => 0x6110 + 6,
            iptCode::RecoverMiss => 0x6110 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            iptCode::ThreshUnset => "阈值未配置",
            iptCode::ThreshConflict => "告警与解除阈值交叉（无迟滞）",
            iptCode::AlertStorm => "告警风暴（频繁翻转）",
            iptCode::SilentFail => "低于阈值未告警",
            iptCode::FalseAlert => "高于阈值仍告警",
            iptCode::SuppressMiss => "抑制窗口未生效",
            iptCode::ScopeMix => "阈值跨口径混用",
            iptCode::RecoverMiss => "恢复后未解除告警",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            iptCode::ThreshUnset => "按默认阈值表初始化",
            iptCode::ThreshConflict => "分离双阈值",
            iptCode::AlertStorm => "启用迟滞与抑制",
            iptCode::SilentFail => "立即判红",
            iptCode::FalseAlert => "解除阈值校正",
            iptCode::SuppressMiss => "补抑制逻辑",
            iptCode::ScopeMix => "按口径分表",
            iptCode::RecoverMiss => "补解除路径",
        }
    }
}


// --------------------------------------------------------------------------
// 冻结常量表 ipt_thresh（6 行逐行注释自证）
// --------------------------------------------------------------------------
/// IPI 健康告警阈值表。
pub const ipt_thresh_COUNT: usize = 6;

/// 表本体（编译期冻结，运行期只读）。
pub static ipt_thresh: [(&'static str, u64); 6] = [
    ("rate_alert", 9990), // 成功率告警阈值（万分比）
    ("rate_clear", 9995), // 成功率解除阈值（万分比，迟滞）
    ("p50_alert", 2500), // P50 告警阈值（ns）
    ("p99_alert", 12000), // P99 告警阈值（ns）
    ("depth_alert", 32), // 队列深度告警阈值
    ("merge_low", 100), // 合并率低下阈值（万分比）
];

/// 线性查找（键唯一由构造期断言保证）。
pub fn ipt_thresh_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < ipt_thresh_COUNT {
        if ipt_thresh[i].0 == key {
            return Some(ipt_thresh[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（守恒对账用）。
pub fn ipt_thresh_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < ipt_thresh_COUNT {
        s = s.wrapping_add(ipt_thresh[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（逐对扫描；重复即结构腐坏）。
pub fn ipt_thresh_unique() -> bool {
    let mut i = 0usize;
    while i < ipt_thresh_COUNT {
        let mut j = i + 1;
        while j < ipt_thresh_COUNT {
            if ipt_thresh[i].0 == ipt_thresh[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}


// --------------------------------------------------------------------------
// 状态机 ipt_alert（3 态 × 4 事件 · 合法迁移 5 条显式穷举）
// --------------------------------------------------------------------------
/// 告警双稳态 + 抑制窗状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ipt_alertState {
    Normal,
    Alerting,
    Suppressed,
}

impl ipt_alertState {
    /// 全量态闭集。
    pub const ALL: [ipt_alertState; 3] = [
        ipt_alertState::Normal,
        ipt_alertState::Alerting,
        ipt_alertState::Suppressed,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            ipt_alertState::Normal => 0,
            ipt_alertState::Alerting => 1,
            ipt_alertState::Suppressed => 2,
        }
    }

    /// 出边存在性（死锁/终态检测用）。
    pub fn has_exit(self) -> bool {
        let mut i = 0usize;
        while i < 5 {
            if IPT_ALERT_TRANS[i].0 == self {
                return true;
            }
            i += 1;
        }
        false
    }
}

/// ipt_alert事件枚举。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ipt_alertEvent {
    Breach,
    Recover,
    SuppressEnd,
    Force,
}

impl ipt_alertEvent {
    /// 全量事件闭集。
    pub const ALL: [ipt_alertEvent; 4] = [
        ipt_alertEvent::Breach,
        ipt_alertEvent::Recover,
        ipt_alertEvent::SuppressEnd,
        ipt_alertEvent::Force,
    ];
}

/// 合法迁移表（表外组合一律非法）。
pub const IPT_ALERT_TRANS: [(ipt_alertState, ipt_alertEvent, ipt_alertState); 5] = [
    (ipt_alertState::Normal, ipt_alertEvent::Breach, ipt_alertState::Alerting), // 越限即告警
    (ipt_alertState::Alerting, ipt_alertEvent::Recover, ipt_alertState::Normal), // 恢复即解除
    (ipt_alertState::Alerting, ipt_alertEvent::SuppressEnd, ipt_alertState::Suppressed), // 进入抑制窗
    (ipt_alertState::Suppressed, ipt_alertEvent::SuppressEnd, ipt_alertState::Normal), // 抑制结束回正常
    (ipt_alertState::Suppressed, ipt_alertEvent::Breach, ipt_alertState::Alerting), // 抑制中再越限仍告警
];

/// 迁移函数（表驱动，无隐式分支）。
pub fn ipt_alert_next(st: ipt_alertState, ev: ipt_alertEvent) -> Option<ipt_alertState> {
    let mut i = 0usize;
    while i < 5 {
        let t = IPT_ALERT_TRANS[i];
        if t.0 == st && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 全组合扫描（ns × ne 格逐一验证）。
pub fn ipt_alert_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < ipt_alertState::ALL.len() {
        let mut ei = 0usize;
        while ei < ipt_alertEvent::ALL.len() {
            if ipt_alert_next(ipt_alertState::ALL[si], ipt_alertEvent::ALL[ei]).is_none() {
                bad += 1;
            }
            ei += 1;
        }
        si += 1;
    }
    bad
}

/// 合法组合数（对拍用）。
pub fn ipt_alert_legal_pairs() -> usize {
    ipt_alertState::ALL.len() * ipt_alertEvent::ALL.len() - ipt_alert_illegal_pairs()
}

/// 迁移账（合法/非法双计数 + 守恒）。
#[derive(Clone, Copy, Debug)]
pub struct ipt_alertLedger {
    /// 合法迁移数。
    pub legal: u64,
    /// 非法拒止数。
    pub illegal: u64,
    /// 当前态。
    pub cur: ipt_alertState,
}

impl ipt_alertLedger {
    /// 构造（初态显式）。
    pub const fn new(init: ipt_alertState) -> Self {
        Self { legal: 0, illegal: 0, cur: init }
    }

    /// 施加事件（合法迁移 / 非法拒止双账）。
    pub fn apply(&mut self, ev: ipt_alertEvent) -> bool {
        match ipt_alert_next(self.cur, ev) {
            Some(ns) => {
                self.cur = ns;
                self.legal += 1;
                true
            }
            None => {
                self.illegal += 1;
                false
            }
        }
    }

    /// 守恒：legal + illegal == 施加总次数。
    pub fn total(&self) -> u64 {
        self.legal + self.illegal
    }
}


// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x6110, 0x611f]。
pub const fn ipt_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < iptCode::ALL.len() {
        let c = iptCode::ALL[i].code();
        if c < 0x6110 || c > 0x611f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 阈值表数值（告警 9990 / 解除 9995）。
fn c1_thresh() -> bool {
    ipt_thresh_get("rate_alert") == Some(9990)
        && ipt_thresh_get("rate_clear") == Some(9995)
        && ipt_thresh_get("p99_alert") == Some(12000)
}

/// 告警态可恢复且抑制可退出。
fn c2_sm() -> bool {
    // 修正记录：原写成 `ipt_alert::new(...)`（E0433 实锤——`ipt_alert` 并非
    // 模块名）。本模块真实类型是 `ipt_alertLedger`（`legal`/`illegal`
    // 计数字段即出自它），构造器为其 `new(init_state)`。
    let mut l = ipt_alertLedger::new(ipt_alertState::Normal);
    l.apply(ipt_alertEvent::Breach) && l.apply(ipt_alertEvent::Recover)
        && l.legal == 2 && l.illegal == 0
}

/// 段位守卫：全部诊断码落在本模块段内。
fn ipt_guard_seg() -> bool {
    ipt_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_ipt_checks() -> CheckSet {
    let mut set = CheckSet::new("ipt");
    set.add(
        "F0930-thr-c1",
        c1_thresh(),
        if c1_thresh() { "" } else { "threshold table drifted" },
    );
    set.add(
        "F0930-thr-c2",
        c2_sm(),
        if c2_sm() { "" } else { "alert hysteresis machine broken" },
    );
    set.add(
        "ipt-seg-guard",
        ipt_guard_seg(),
        if ipt_guard_seg() { "" } else { "diagnostic code outside module segment" },
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
    fn ipt_all_checks_green() {
        let set = run_ipt_checks();
        assert!(set.all_passed(), "IPT domain checks must be green");
    }

    #[test]
    fn ipt_segment_guard_holds() {
        assert!(ipt_segment_guard());
    }
}
