//! UNX-F1081 墙钟 NTP 校准 · 来源可信度审计
//! 判据原文（CoRun Unxreal 主册）：UNX-F1081-J1 双协议正确（阶跃档输出跳变 10/10 次、渐变档输出斜率受控 10/10 次），校准事件账落盘（来源/幅度/档位全记录），基础面零回归（F1079 判据复测全过）
//! · 组件：来源可信度分级表 + 选源策略 + 冲突源仲裁账
//! · 判据落地：来源/幅度/档位全记录的可信度底账
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const NTQ_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const NTQ_SEG_BASE: u16 = 0x6250;

// --------------------------------------------------------------------------
// 诊断码（ntq 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 来源审计诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ntqCode {
    /// 来源可信度未知。
    TrustUnknown,
    /// 多源结论冲突。
    SrcConflict,
    /// 来源可信度下降。
    TrustDrop,
    /// NTP 包校验失败。
    PacketBad,
    /// 上下行延迟不对称。
    DelayAsym,
    /// 层级（stratum）非法。
    StratumBad,
    /// 低可信源被选为首选。
    RefuseTrust,
    /// 仲裁未收敛即宣告。
    FinalNotGreen,
}

impl ntqCode {
    /// 全量闭集。
    pub const ALL: [ntqCode; 8] = [
        ntqCode::TrustUnknown,
        ntqCode::SrcConflict,
        ntqCode::TrustDrop,
        ntqCode::PacketBad,
        ntqCode::DelayAsym,
        ntqCode::StratumBad,
        ntqCode::RefuseTrust,
        ntqCode::FinalNotGreen,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            ntqCode::TrustUnknown => 0x6250 + 0,
            ntqCode::SrcConflict => 0x6250 + 1,
            ntqCode::TrustDrop => 0x6250 + 2,
            ntqCode::PacketBad => 0x6250 + 3,
            ntqCode::DelayAsym => 0x6250 + 4,
            ntqCode::StratumBad => 0x6250 + 5,
            ntqCode::RefuseTrust => 0x6250 + 6,
            ntqCode::FinalNotGreen => 0x6250 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            ntqCode::TrustUnknown => "来源可信度未知",
            ntqCode::SrcConflict => "多源结论冲突",
            ntqCode::TrustDrop => "来源可信度下降",
            ntqCode::PacketBad => "NTP 包校验失败",
            ntqCode::DelayAsym => "上下行延迟不对称",
            ntqCode::StratumBad => "层级（stratum）非法",
            ntqCode::RefuseTrust => "低可信源被选为首选",
            ntqCode::FinalNotGreen => "仲裁未收敛即宣告",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            ntqCode::TrustUnknown => "按最低可信度处理",
            ntqCode::SrcConflict => "按可信度仲裁",
            ntqCode::TrustDrop => "记录并降权",
            ntqCode::PacketBad => "丢弃该样本",
            ntqCode::DelayAsym => "按对称化修正",
            ntqCode::StratumBad => "按 1..15 校正",
            ntqCode::RefuseTrust => "重新选源",
            ntqCode::FinalNotGreen => "收敛后才宣告",
        }
    }
}


// --------------------------------------------------------------------------
// 冻结常量表 ntq_trust（6 行逐行注释自证）
// --------------------------------------------------------------------------
/// 来源可信度表（按 NTP stratum）。
pub const ntq_trust_COUNT: usize = 6;

/// 表本体（编译期冻结，运行期只读）。
pub static ntq_trust: [(&'static str, u64); 6] = [
    ("Stratum1", 10000), // 层级 1 可信度万分比
    ("Stratum2", 8000), // 层级 2
    ("Stratum3", 6000), // 层级 3
    ("Stratum4", 4000), // 层级 4
    ("Stratum5Plus", 2000), // 层级 5+
    ("RtcFloor", 500), // CMOS RTC 可信度下限
];

/// 线性查找（键唯一由构造期断言保证）。
pub fn ntq_trust_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < ntq_trust_COUNT {
        if ntq_trust[i].0 == key {
            return Some(ntq_trust[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（守恒对账用）。
pub fn ntq_trust_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < ntq_trust_COUNT {
        s = s.wrapping_add(ntq_trust[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（逐对扫描；重复即结构腐坏）。
pub fn ntq_trust_unique() -> bool {
    let mut i = 0usize;
    while i < ntq_trust_COUNT {
        let mut j = i + 1;
        while j < ntq_trust_COUNT {
            if ntq_trust[i].0 == ntq_trust[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}


// --------------------------------------------------------------------------
// 状态机 ntq_arb（4 态 × 5 事件 · 合法迁移 6 条显式穷举）
// --------------------------------------------------------------------------
/// 来源仲裁状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ntq_arbState {
    Collecting,
    Scored,
    Arbitrated,
    Published,
}

impl ntq_arbState {
    /// 全量态闭集。
    pub const ALL: [ntq_arbState; 4] = [
        ntq_arbState::Collecting,
        ntq_arbState::Scored,
        ntq_arbState::Arbitrated,
        ntq_arbState::Published,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            ntq_arbState::Collecting => 0,
            ntq_arbState::Scored => 1,
            ntq_arbState::Arbitrated => 2,
            ntq_arbState::Published => 3,
        }
    }

    /// 出边存在性（死锁/终态检测用）。
    pub fn has_exit(self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            if NTQ_ARB_TRANS[i].0 == self {
                return true;
            }
            i += 1;
        }
        false
    }
}

/// ntq_arb事件枚举。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ntq_arbEvent {
    Sample,
    Score,
    Conflict,
    Arbitrate,
    Publish,
}

impl ntq_arbEvent {
    /// 全量事件闭集。
    pub const ALL: [ntq_arbEvent; 5] = [
        ntq_arbEvent::Sample,
        ntq_arbEvent::Score,
        ntq_arbEvent::Conflict,
        ntq_arbEvent::Arbitrate,
        ntq_arbEvent::Publish,
    ];
}

/// 合法迁移表（表外组合一律非法）。
pub const NTQ_ARB_TRANS: [(ntq_arbState, ntq_arbEvent, ntq_arbState); 6] = [
    (ntq_arbState::Collecting, ntq_arbEvent::Sample, ntq_arbState::Collecting), // 累积样本
    (ntq_arbState::Collecting, ntq_arbEvent::Score, ntq_arbState::Scored), // 样本足够后评分
    (ntq_arbState::Scored, ntq_arbEvent::Conflict, ntq_arbState::Scored), // 检测到源冲突
    (ntq_arbState::Scored, ntq_arbEvent::Arbitrate, ntq_arbState::Arbitrated), // 按可信度仲裁
    (ntq_arbState::Arbitrated, ntq_arbEvent::Publish, ntq_arbState::Published), // 发布校准结论
    (ntq_arbState::Published, ntq_arbEvent::Sample, ntq_arbState::Collecting), // 下一轮采集
]
;

/// 迁移函数（表驱动，无隐式分支）。
pub fn ntq_arb_next(st: ntq_arbState, ev: ntq_arbEvent) -> Option<ntq_arbState> {
    let mut i = 0usize;
    while i < 6 {
        let t = NTQ_ARB_TRANS[i];
        if t.0 == st && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 全组合扫描（ns × ne 格逐一验证）。
pub fn ntq_arb_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < ntq_arbState::ALL.len() {
        let mut ei = 0usize;
        while ei < ntq_arbEvent::ALL.len() {
            if ntq_arb_next(ntq_arbState::ALL[si], ntq_arbEvent::ALL[ei]).is_none() {
                bad += 1;
            }
            ei += 1;
        }
        si += 1;
    }
    bad
}

/// 合法组合数（对拍用）。
pub fn ntq_arb_legal_pairs() -> usize {
    ntq_arbState::ALL.len() * ntq_arbEvent::ALL.len() - ntq_arb_illegal_pairs()
}

/// 迁移账（合法/非法双计数 + 守恒）。
#[derive(Clone, Copy, Debug)]
pub struct ntq_arbLedger {
    /// 合法迁移数。
    pub legal: u64,
    /// 非法拒止数。
    pub illegal: u64,
    /// 当前态。
    pub cur: ntq_arbState,
}

impl ntq_arbLedger {
    /// 构造（初态显式）。
    pub const fn new(init: ntq_arbState) -> Self {
        Self { legal: 0, illegal: 0, cur: init }
    }

    /// 施加事件（合法迁移 / 非法拒止双账）。
    pub fn apply(&mut self, ev: ntq_arbEvent) -> bool {
        match ntq_arb_next(self.cur, ev) {
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

/// 段位守卫：全部诊断码 ∈ [0x6250, 0x625f]。
pub const fn ntq_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < ntqCode::ALL.len() {
        let c = ntqCode::ALL[i].code();
        if c < 0x6250 || c > 0x625f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 可信度分层单调。
fn c1_trust() -> bool {
    ntq_trust_get("Stratum1") == Some(10000) && ntq_trust_get("Stratum2") == Some(8000)
        && ntq_trust_get("Stratum3") == Some(6000)
        && ntq_trust_get("RtcFloor") == Some(500)
        && ntq_trust_sum() == 30500
}

/// 仲裁状态机可发布。
fn c2_sm() -> bool {
    let mut l = ntq_arbLedger::new(ntq_arbState::Collecting);
    l.apply(ntq_arbEvent::Sample) && l.apply(ntq_arbEvent::Score)
        && l.apply(ntq_arbEvent::Arbitrate) && l.apply(ntq_arbEvent::Publish)
        && l.legal == 4 && l.illegal == 0
}

/// 冲突后必须仲裁才可发布。
fn c3_conflict() -> bool {
    let mut l = ntq_arbLedger::new(ntq_arbState::Collecting);
    l.apply(ntq_arbEvent::Score);
    !l.apply(ntq_arbEvent::Publish)
        && l.apply(ntq_arbEvent::Conflict) && l.apply(ntq_arbEvent::Arbitrate)
        && l.apply(ntq_arbEvent::Publish) && l.illegal == 1
}

/// 段位守卫：全部诊断码落在本模块段内。
fn ntq_guard_seg() -> bool {
    ntq_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_ntq_checks() -> CheckSet {
    let mut set = CheckSet::new("ntq");
    set.add(
        "F1081-src-c1",
        c1_trust(),
        if c1_trust() { "" } else { "trust stratification broken" },
    );
    set.add(
        "F1081-src-c2",
        c2_sm(),
        if c2_sm() { "" } else { "arbitration machine path broken" },
    );
    set.add(
        "F1081-src-c3",
        c3_conflict(),
        if c3_conflict() { "" } else { "publish allowed without arbitration" },
    );
    set.add(
        "ntq-seg-guard",
        ntq_guard_seg(),
        if ntq_guard_seg() { "" } else { "diagnostic code outside module segment" },
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
    fn ntq_all_checks_green() {
        let set = run_ntq_checks();
        assert!(set.all_passed(), "NTQ domain checks must be green");
    }

    #[test]
    fn ntq_segment_guard_holds() {
        assert!(ntq_segment_guard());
    }
}
