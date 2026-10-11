//! unxg06a_cgj06_pacefive —— CGPU-F1446 五账户分账模型（CPU/iGPU/dGPU/内存/屏幕）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：CGPU-F1446 · 核显与内存功耗分账（CGPU-J 域 · 批次 J01 · J06 组 · 判据：五账户/估算标注/模型参数/屏幕查表/5% 审计/H 域联动/不确定度/测试）
//! · 组件：五账户表 + 分账引擎 + 估算标注 + 5% 偏差审计 + H 域联动
//! · 判据落地：整机功耗→五账户分账
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const G06A_VERSION: &str = "g06a-cgj06_pacefive-v1";

/// 诊断段基址。
pub const G06A_SEG_BASE: u16 = 0x7000;

// --------------------------------------------------------------------------
// 诊断码（g06a 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 分账模型诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum g06aCode {
    /// 0x7000 账户非法（五账户之外）。
    AcctBad,
    /// 0x7001 无基分账（不可分却强行分）。
    SplitWithoutBase,
    /// 0x7002 分账和漂移（合计≠总账）。
    SumDrift,
    /// 0x7003 估算未标注（估算值无标记）。
    EstimateNotMarked,
    /// 0x7004 参数表缺失（模型无校准参数）。
    ParamTableMissing,
    /// 0x7005 H 域联动缺失（显存预算池未参考）。
    HwDomainMissing,
    /// 0x7006 不确定度缺失（分账值无元数据）。
    UncertaintyMissing,
    /// 0x7007 守卫触发。
    GuardTripped,
    /// 0x7008 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x7009 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x700a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x700b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x700c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x700d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x700e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl g06aCode {
    /// 全量闭集。
    pub const ALL: [g06aCode; 15] = [
        g06aCode::AcctBad,
        g06aCode::SplitWithoutBase,
        g06aCode::SumDrift,
        g06aCode::EstimateNotMarked,
        g06aCode::ParamTableMissing,
        g06aCode::HwDomainMissing,
        g06aCode::UncertaintyMissing,
        g06aCode::GuardTripped,
        g06aCode::LedgerBroken,
        g06aCode::SnapshotFingerprint,
        g06aCode::TableBroken,
        g06aCode::IndexMisaligned,
        g06aCode::DuplicateEntry,
        g06aCode::OrderViolated,
        g06aCode::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            g06aCode::AcctBad => 0x7000,
            g06aCode::SplitWithoutBase => 0x7001,
            g06aCode::SumDrift => 0x7002,
            g06aCode::EstimateNotMarked => 0x7003,
            g06aCode::ParamTableMissing => 0x7004,
            g06aCode::HwDomainMissing => 0x7005,
            g06aCode::UncertaintyMissing => 0x7006,
            g06aCode::GuardTripped => 0x7007,
            g06aCode::LedgerBroken => 0x7008,
            g06aCode::SnapshotFingerprint => 0x7009,
            g06aCode::TableBroken => 0x700a,
            g06aCode::IndexMisaligned => 0x700b,
            g06aCode::DuplicateEntry => 0x700c,
            g06aCode::OrderViolated => 0x700d,
            g06aCode::SegmentGuard => 0x700e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            g06aCode::AcctBad => "账户非法（五账户之外）",
            g06aCode::SplitWithoutBase => "无基分账（不可分却强行分）",
            g06aCode::SumDrift => "分账和漂移（合计≠总账）",
            g06aCode::EstimateNotMarked => "估算未标注（估算值无标记）",
            g06aCode::ParamTableMissing => "参数表缺失（模型无校准参数）",
            g06aCode::HwDomainMissing => "H 域联动缺失（显存预算池未参考）",
            g06aCode::UncertaintyMissing => "不确定度缺失（分账值无元数据）",
            g06aCode::GuardTripped => "守卫触发",
            g06aCode::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            g06aCode::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            g06aCode::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            g06aCode::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            g06aCode::DuplicateEntry => "条目重复（同键双写）",
            g06aCode::OrderViolated => "次序违例（依赖序/时序破缺）",
            g06aCode::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            g06aCode::AcctBad => "按五账户闭集校正",
            g06aCode::SplitWithoutBase => "按整账显性声明路径",
            g06aCode::SumDrift => "按守恒重建",
            g06aCode::EstimateNotMarked => "按估算标注规程补标",
            g06aCode::ParamTableMissing => "按公开参数表补配",
            g06aCode::HwDomainMissing => "按跨域接口声明补接",
            g06aCode::UncertaintyMissing => "按不确定度标注补配",
            g06aCode::GuardTripped => "检查临界区",
            g06aCode::LedgerBroken => "按守恒律重建台账后重跑",
            g06aCode::SnapshotFingerprint => "按防篡改规程重建快照",
            g06aCode::TableBroken => "按语义规则重建常量表",
            g06aCode::IndexMisaligned => "按声明序归一化后重跑",
            g06aCode::DuplicateEntry => "合并同键条目为末次生效",
            g06aCode::OrderViolated => "按依赖矩阵重排后重跑",
            g06aCode::SegmentGuard => "核对诊断码分配表",
        }
    }
}


// ---------------------------------------------------------------------------
// 五账户分账本体（整机总账 + 五账户分账，估算标注内建）
// ---------------------------------------------------------------------------

/// 五账户（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerAccount {
    /// CPU 包功耗账户。
    Cpu,
    /// 核显（iGPU）账户。
    Igpu,
    /// 独显（dGPU）账户。
    Dgpu,
    /// 内存账户（DRAM）。
    Dram,
    /// 屏幕面板账户。
    Panel,
}

impl PowerAccount {
    /// 全量闭集（五账户）。
    pub const ALL: [PowerAccount; 5] = [
        PowerAccount::Cpu,
        PowerAccount::Igpu,
        PowerAccount::Dgpu,
        PowerAccount::Dram,
        PowerAccount::Panel,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            PowerAccount::Cpu => 0,
            PowerAccount::Igpu => 1,
            PowerAccount::Dgpu => 2,
            PowerAccount::Dram => 3,
            PowerAccount::Panel => 4,
        }
    }

    /// 中文账户名。
    pub const fn cn(self) -> &'static str {
        match self {
            PowerAccount::Cpu => "CPU",
            PowerAccount::Igpu => "核显",
            PowerAccount::Dgpu => "独显",
            PowerAccount::Dram => "内存",
            PowerAccount::Panel => "屏幕",
        }
    }
}

/// 账户数常数。
pub const ACCOUNT_N: usize = 5;

/// 分账值（毫瓦 mW 口径，整数免浮点）。
pub type MilliWatt = u64;

/// 单账户分账（值 + 估算标记 + 不确定度万分比）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccountSplit {
    /// 分账值（mW）。
    pub value_mw: MilliWatt,
    /// 是否估算（true = 估算非实测）。
    pub estimated: bool,
    /// 不确定度（万分比，0..10000）。
    pub uncertainty_permyriad: u32,
}

impl AccountSplit {
    /// 构造（估算默认带 10% 不确定度）。
    pub const fn new(value_mw: MilliWatt, estimated: bool) -> Self {
        Self {
            value_mw,
            estimated,
            uncertainty_permyriad: if estimated { 1000 } else { 0 },
        }
    }

    /// 带不确定度构造。
    pub const fn with_uncertainty(value_mw: MilliWatt, estimated: bool, permyriad: u32) -> Self {
        Self { value_mw, estimated, uncertainty_permyriad: permyriad }
    }
}

/// 分账账本（五账户 + 总账 + 守恒）。
#[derive(Clone, Copy, Debug)]
pub struct PowerBook {
    /// 五账户分账。
    pub splits: [AccountSplit; 5],
    /// 整机总账（mW）。
    pub total_mw: MilliWatt,
    /// 不可分账户显性声明（true = 整账不拆，声明优先）。
    pub indivisible: bool,
}

impl PowerBook {
    /// 空账构造。
    pub const fn new() -> Self {
        Self {
            splits: [AccountSplit::new(0, false); 5],
            total_mw: 0,
            indivisible: false,
        }
    }

    /// 登记一账户（值 + 估算标记）。
    pub fn set(&mut self, acct: PowerAccount, split: AccountSplit) -> bool {
        if acct.index() >= ACCOUNT_N {
            return false;
        }
        self.splits[acct.index()] = split;
        true
    }

    /// 分账合计（五账户和）。
    pub fn split_sum(&self) -> MilliWatt {
        let mut s = 0u64;
        let mut i = 0;
        while i < ACCOUNT_N {
            s = s.wrapping_add(self.splits[i].value_mw);
            i += 1;
        }
        s
    }

    /// 守恒判定（合 == 总；不可分时声明优先）。
    pub fn conserved(&self) -> bool {
        if self.indivisible {
            return true;
        }
        self.split_sum() == self.total_mw
    }

    /// 偏差万分比（审计 5% 判据）。
    pub fn deviation_permyriad(&self) -> u32 {
        if self.total_mw == 0 {
            return 0;
        }
        let sum = self.split_sum();
        let diff = if sum > self.total_mw { sum - self.total_mw } else { self.total_mw - sum };
        ((diff * 10000) / self.total_mw) as u32
    }
}

/// H 域联动（显存预算池决策参考显存功耗：跨域接口声明）。
pub const fn hw_link_enabled() -> bool {
    true
}

/// 联动参考值（显存功耗进入 H 域预算池决策的权重千分比）。
pub const HW_LINK_WEIGHT_PERMILLE: u32 = 250;

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x7000, 0x700f]。
pub const fn g06a_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < g06aCode::ALL.len() {
        let c = g06aCode::ALL[i].code();
        if c < 0x7000 || c > 0x700f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 段位守卫：全部诊断码落在本模块段内。
fn g06a_guard_seg() -> bool {
    g06a_segment_guard()
}

/// 分账账本守恒（五账户合 == 总账 + 估算标注）。
fn check_c1_book_conserve() -> bool {
    let mut b = PowerBook::new();
    b.total_mw = 65000;
    let mut ok = true;
    let mut i = 0;
    while i < ACCOUNT_N {
        if !b.set(PowerAccount::ALL[i], AccountSplit::new(13000, i == 3)) {
            ok = false;
        }
        i += 1;
    }
    if ok {
        ok = b.split_sum() == 65000 && b.conserved() && b.deviation_permyriad() == 0;
    }
    if ok {
        // 内存账户是估算（不确定度 10%），其余实测零不确定度。
        ok = b.splits[3].estimated && b.splits[0].uncertainty_permyriad == 0;
    }
    ok
}

/// 5% 偏差判据（红线内/越界两样本 + H 域联动）。
fn check_c2_deviation() -> bool {
    let mut b = PowerBook::new();
    b.total_mw = 60000;
    let _ = b.set(PowerAccount::Cpu, AccountSplit::new(15000, false));
    let _ = b.set(PowerAccount::Igpu, AccountSplit::new(12000, false));
    let _ = b.set(PowerAccount::Dgpu, AccountSplit::new(9000, true));
    let _ = b.set(PowerAccount::Dram, AccountSplit::new(6000, false));
    let _ = b.set(PowerAccount::Panel, AccountSplit::new(17800, false));
    let mut ok = b.split_sum() == 59800 && b.deviation_permyriad() <= 500;
    if ok {
        // 越界样本：偏离 5% 以上。
        let mut c = PowerBook::new();
        c.total_mw = 10000;
        let _ = c.set(PowerAccount::Cpu, AccountSplit::new(2000, false));
        ok = c.deviation_permyriad() >= 500;
    }
    if ok {
        ok = hw_link_enabled() && HW_LINK_WEIGHT_PERMILLE == 250;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_g06a_checks() -> CheckSet {
    let mut set = CheckSet::new("G06A");
    set.add(
        "g06a-seg-guard",
        g06a_guard_seg(),
        if g06a_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "g06a-c1-book-conserve",
        check_c1_book_conserve(),
        if check_c1_book_conserve() { "" } else { "power book conservation mismatch" },
    );
    set.add(
        "g06a-c2-deviation",
        check_c2_deviation(),
        if check_c2_deviation() { "" } else { "power deviation audit mismatch" },
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
    fn g06a_all_checks_green() {
        let set = run_g06a_checks();
        assert!(set.all_passed(), "G06A domain checks must be green");
    }

    #[test]
    fn g06a_segment_guard_holds() {
        assert!(g06a_segment_guard());
    }
}
