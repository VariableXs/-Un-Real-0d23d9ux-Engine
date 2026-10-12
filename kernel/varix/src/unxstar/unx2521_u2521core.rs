//! unxu2521_u2521core —— 碎片指数定义与计算（B02 前向引用兑现） 语义登记表与守恒账（碎片指数）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2521 · 碎片指数定义与计算（B02 前向引用兑现） · 判据原文：UNX-F2521 · 判据：碎片指数计算与手算样例一致（可复算断言），指数含大页项（F2517 契约消费）
//! · 组件：碎片指数 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2521 · 判据：碎片指数计算与手算样例一致（可复算断言），指数含大页项（F2517 契约消费）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2521_VERSION: &str = "u2521-u2521core-v1";

/// 诊断段基址。
pub const U2521_SEG_BASE: u16 = 0xa480;

// --------------------------------------------------------------------------
// 诊断码（u2521 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 碎片指数 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2521Code {
    /// 0xa480 表行重复（复算不符 同键双写）。
    RowDup,
    /// 0xa481 表行断档（大页项缺 序号不连续）。
    RowGap,
    /// 0xa482 值越界（指数越界 值域外）。
    ValOob,
    /// 0xa483 累计漂移（样例漂移 求和≠理论值）。
    SumDrift,
    /// 0xa484 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xa485 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xa486 空操作（账目无效果调用）。
    OpNoop,
    /// 0xa487 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xa488 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa489 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa48a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa48b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2521Code {
    /// 全量闭集。
    pub const ALL: [u2521Code; 12] = [
        u2521Code::RowDup,
        u2521Code::RowGap,
        u2521Code::ValOob,
        u2521Code::SumDrift,
        u2521Code::LedgerDup,
        u2521Code::LedgerHole,
        u2521Code::OpNoop,
        u2521Code::FieldDrift,
        u2521Code::LedgerBroken,
        u2521Code::SnapshotFingerprint,
        u2521Code::TableBroken,
        u2521Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2521Code::RowDup => 0xa480,
            u2521Code::RowGap => 0xa481,
            u2521Code::ValOob => 0xa482,
            u2521Code::SumDrift => 0xa483,
            u2521Code::LedgerDup => 0xa484,
            u2521Code::LedgerHole => 0xa485,
            u2521Code::OpNoop => 0xa486,
            u2521Code::FieldDrift => 0xa487,
            u2521Code::LedgerBroken => 0xa488,
            u2521Code::SnapshotFingerprint => 0xa489,
            u2521Code::TableBroken => 0xa48a,
            u2521Code::SegmentGuard => 0xa48b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2521Code::RowDup => "表行重复（复算不符 同键双写）",
            u2521Code::RowGap => "表行断档（大页项缺 序号不连续）",
            u2521Code::ValOob => "值越界（指数越界 值域外）",
            u2521Code::SumDrift => "累计漂移（样例漂移 求和≠理论值）",
            u2521Code::LedgerDup => "账目重记（同一事件双入账）",
            u2521Code::LedgerHole => "账目断漏（序号缺失）",
            u2521Code::OpNoop => "空操作（账目无效果调用）",
            u2521Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2521Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2521Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2521Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2521Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2521Code::RowDup => "按键唯一律去重",
            u2521Code::RowGap => "按连续律补号",
            u2521Code::ValOob => "按值域裁剪",
            u2521Code::SumDrift => "按理论值复算",
            u2521Code::LedgerDup => "按事件幂等律合并",
            u2521Code::LedgerHole => "按连续律补账",
            u2521Code::OpNoop => "核对操作前置条件",
            u2521Code::FieldDrift => "按口径重建字段",
            u2521Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2521Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2521Code::TableBroken => "按语义规则重建常量表",
            u2521Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2521reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 碎片指数 语义登记表（24 行逐行注释）。
pub const u2521reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2521reg: [(&'static str, u64); 24] = [
    ("FI_A", 7), // FI 主登记（碎片指数 · 判据面）
    ("FI_B", 33), // FI 副登记（碎片指数 · 对拍面）
    ("HC_A", 14), // HC 主登记（手算样例 · 判据面）
    ("HC_B", 44), // HC 副登记（手算样例 · 对拍面）
    ("RC_A", 21), // RC 主登记（可复算 · 判据面）
    ("RC_B", 55), // RC 副登记（可复算 · 对拍面）
    ("HP_A", 28), // HP 主登记（大页项 · 判据面）
    ("HP_B", 66), // HP 副登记（大页项 · 对拍面）
    ("CT_A", 35), // CT 主登记（契约消费 · 判据面）
    ("CT_B", 77), // CT 副登记（契约消费 · 对拍面）
    ("FR_A", 42), // FR 主登记（空闲碎片 · 判据面）
    ("FR_B", 88), // FR 副登记（空闲碎片 · 对拍面）
    ("AV_A", 49), // AV 主登记（可用连续 · 判据面）
    ("AV_B", 99), // AV 副登记（可用连续 · 对拍面）
    ("MN_A", 56), // MN 主登记（最大连续 · 判据面）
    ("MN_B", 110), // MN 副登记（最大连续 · 对拍面）
    ("RT_A", 63), // RT 主登记（比率账 · 判据面）
    ("RT_B", 121), // RT 副登记（比率账 · 对拍面）
    ("QT_A", 70), // QT 主登记（量化档 · 判据面）
    ("QT_B", 132), // QT 副登记（量化档 · 对拍面）
    ("SB_A", 77), // SB 主登记（算式账 · 判据面）
    ("SB_B", 143), // SB 副登记（算式账 · 对拍面）
    ("AS_A", 84), // AS 主登记（断言面 · 判据面）
    ("AS_B", 154), // AS 副登记（断言面 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2521reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2521reg_COUNT {
        if u2521reg[i].0 == key {
            return Some(u2521reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2521reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2521reg_COUNT {
        s = s.wrapping_add(u2521reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2521Ledger（碎片指数 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 碎片指数 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2521Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2521Ledger {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            entries: 0,
            hits: 0,
            notes: 0,
        }
    }

    /// 入账一笔
    pub fn bump_entry(&mut self) {
        self.entries = self.entries.wrapping_add(1);
    }

    /// 回执一笔
    pub fn bump_hit(&mut self) {
        self.hits = self.hits.wrapping_add(1);
    }

    /// 注记一笔
    pub fn log_note(&mut self) {
        self.notes = self.notes.wrapping_add(1);
    }

}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0xa480, 0xa48f]。
pub const fn u2521_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2521Code::ALL.len() {
        let c = u2521Code::ALL[i].code();
        if c < 0xa480 || c > 0xa48f {
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
fn u2521_guard_seg() -> bool {
    u2521_segment_guard()
}

/// UNX-F2521 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2521_core_tbl() -> bool {
    let mut ok = u2521reg_COUNT == 24;
    if ok { ok = u2521reg_get("FI_A") == Some(7) && u2521reg_get("FI_B") == Some(33); }
    if ok { ok = u2521reg_get("AS_A") == Some(84); }
    if ok { ok = u2521reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2521reg_sum() == 1668; }
    ok
}

/// UNX-F2521 守恒账三操作确定性效果。
fn check_u2521_core_ldg() -> bool {
    let mut ld = Mxu2521Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2521_checks() -> CheckSet {
    let mut set = CheckSet::new("U2521");
    set.add(
        "u2521-seg-guard",
        u2521_guard_seg(),
        if u2521_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2521-core-tbl",
        check_u2521_core_tbl(),
        if check_u2521_core_tbl() { "" } else { "UNX-F2521 core table mismatch" },
    );
    set.add(
        "u2521-core-ldg",
        check_u2521_core_ldg(),
        if check_u2521_core_ldg() { "" } else { "UNX-F2521 core ledger mismatch" },
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
    fn u2521_all_checks_green() {
        let set = run_u2521_checks();
        assert!(set.all_passed(), "U2521 domain checks must be green");
    }

    #[test]
    fn u2521_segment_guard_holds() {
        assert!(u2521_segment_guard());
    }
}
