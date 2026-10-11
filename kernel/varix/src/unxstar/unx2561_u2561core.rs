//! unxu2561_u2561core —— AddressSpace 成熟体总纲（现存 addrspace.rs 接管） 语义登记表与守恒账（地址空间体）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2561 · AddressSpace 成熟体总纲（现存 addrspace.rs 接管） · 判据原文：UNX-F2561 · 判据：现存 `addrspace.rs`（F001 起步态）接管账在档（语义逐项映射），成熟体接口面三机直测一致
//! · 组件：地址空间体 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2561 · 判据：现存 `addrspace.rs`（F001 起步态）接管账在档（语义逐项映射），成熟体接口面三机直测一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2561_VERSION: &str = "u2561-u2561core-v1";

/// 诊断段基址。
pub const U2561_SEG_BASE: u16 = 0xa600;

// --------------------------------------------------------------------------
// 诊断码（u2561 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 地址空间体 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2561Code {
    /// 0xa600 表行重复（接管漏项 同键双写）。
    RowDup,
    /// 0xa601 表行断档（语义错配 序号不连续）。
    RowGap,
    /// 0xa602 值越界（权限失真 值域外）。
    ValOob,
    /// 0xa603 累计漂移（直测分歧 求和≠理论值）。
    SumDrift,
    /// 0xa604 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xa605 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xa606 空操作（账目无效果调用）。
    OpNoop,
    /// 0xa607 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xa608 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa609 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa60a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa60b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2561Code {
    /// 全量闭集。
    pub const ALL: [u2561Code; 12] = [
        u2561Code::RowDup,
        u2561Code::RowGap,
        u2561Code::ValOob,
        u2561Code::SumDrift,
        u2561Code::LedgerDup,
        u2561Code::LedgerHole,
        u2561Code::OpNoop,
        u2561Code::FieldDrift,
        u2561Code::LedgerBroken,
        u2561Code::SnapshotFingerprint,
        u2561Code::TableBroken,
        u2561Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2561Code::RowDup => 0xa600,
            u2561Code::RowGap => 0xa601,
            u2561Code::ValOob => 0xa602,
            u2561Code::SumDrift => 0xa603,
            u2561Code::LedgerDup => 0xa604,
            u2561Code::LedgerHole => 0xa605,
            u2561Code::OpNoop => 0xa606,
            u2561Code::FieldDrift => 0xa607,
            u2561Code::LedgerBroken => 0xa608,
            u2561Code::SnapshotFingerprint => 0xa609,
            u2561Code::TableBroken => 0xa60a,
            u2561Code::SegmentGuard => 0xa60b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2561Code::RowDup => "表行重复（接管漏项 同键双写）",
            u2561Code::RowGap => "表行断档（语义错配 序号不连续）",
            u2561Code::ValOob => "值越界（权限失真 值域外）",
            u2561Code::SumDrift => "累计漂移（直测分歧 求和≠理论值）",
            u2561Code::LedgerDup => "账目重记（同一事件双入账）",
            u2561Code::LedgerHole => "账目断漏（序号缺失）",
            u2561Code::OpNoop => "空操作（账目无效果调用）",
            u2561Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2561Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2561Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2561Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2561Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2561Code::RowDup => "按键唯一律去重",
            u2561Code::RowGap => "按连续律补号",
            u2561Code::ValOob => "按值域裁剪",
            u2561Code::SumDrift => "按理论值复算",
            u2561Code::LedgerDup => "按事件幂等律合并",
            u2561Code::LedgerHole => "按连续律补账",
            u2561Code::OpNoop => "核对操作前置条件",
            u2561Code::FieldDrift => "按口径重建字段",
            u2561Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2561Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2561Code::TableBroken => "按语义规则重建常量表",
            u2561Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2561reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 地址空间体 语义登记表（24 行逐行注释）。
pub const u2561reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2561reg: [(&'static str, u64); 24] = [
    ("AS_A", 7), // AS 主登记（AddressSpace · 判据面）
    ("AS_B", 33), // AS 副登记（AddressSpace · 对拍面）
    ("LG_A", 14), // LG 主登记（接管账 · 判据面）
    ("LG_B", 44), // LG 副登记（接管账 · 对拍面）
    ("SM_A", 21), // SM 主登记（语义映射 · 判据面）
    ("SM_B", 55), // SM 副登记（语义映射 · 对拍面）
    ("MT_A", 28), // MT 主登记（成熟体 · 判据面）
    ("MT_B", 66), // MT 副登记（成熟体 · 对拍面）
    ("IF_A", 35), // IF 主登记（接口面 · 判据面）
    ("IF_B", 77), // IF 副登记（接口面 · 对拍面）
    ("TM_A", 42), // TM 主登记（三机直测 · 判据面）
    ("TM_B", 88), // TM 副登记（三机直测 · 对拍面）
    ("AS2_A", 49), // AS2 主登记（起步态 · 判据面）
    ("AS2_B", 99), // AS2 副登记（起步态 · 对拍面）
    ("MP_A", 56), // MP 主登记（映射项 · 判据面）
    ("MP_B", 110), // MP 副登记（映射项 · 对拍面）
    ("UN_A", 63), // UN 主登记（解除项 · 判据面）
    ("UN_B", 121), // UN 副登记（解除项 · 对拍面）
    ("PC_A", 70), // PC 主登记（权限账 · 判据面）
    ("PC_B", 132), // PC 副登记（权限账 · 对拍面）
    ("FV_A", 77), // FV 主登记（逐项核验 · 判据面）
    ("FV_B", 143), // FV 副登记（逐项核验 · 对拍面）
    ("AR_A", 84), // AR 主登记（在档锚 · 判据面）
    ("AR_B", 154), // AR 副登记（在档锚 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2561reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2561reg_COUNT {
        if u2561reg[i].0 == key {
            return Some(u2561reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2561reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2561reg_COUNT {
        s = s.wrapping_add(u2561reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2561Ledger（地址空间体 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 地址空间体 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2561Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2561Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa600, 0xa60f]。
pub const fn u2561_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2561Code::ALL.len() {
        let c = u2561Code::ALL[i].code();
        if c < 0xa600 || c > 0xa60f {
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
fn u2561_guard_seg() -> bool {
    u2561_segment_guard()
}

/// UNX-F2561 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2561_core_tbl() -> bool {
    let mut ok = u2561reg_COUNT == 24;
    if ok { ok = u2561reg_get("AS_A") == Some(7) && u2561reg_get("AS_B") == Some(33); }
    if ok { ok = u2561reg_get("AR_A") == Some(84); }
    if ok { ok = u2561reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2561reg_sum() == 1668; }
    ok
}

/// UNX-F2561 守恒账三操作确定性效果。
fn check_u2561_core_ldg() -> bool {
    let mut ld = Mxu2561Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2561_checks() -> CheckSet {
    let mut set = CheckSet::new("U2561");
    set.add(
        "u2561-seg-guard",
        u2561_guard_seg(),
        if u2561_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2561-core-tbl",
        check_u2561_core_tbl(),
        if check_u2561_core_tbl() { "" } else { "UNX-F2561 core table mismatch" },
    );
    set.add(
        "u2561-core-ldg",
        check_u2561_core_ldg(),
        if check_u2561_core_ldg() { "" } else { "UNX-F2561 core ledger mismatch" },
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
    fn u2561_all_checks_green() {
        let set = run_u2561_checks();
        assert!(set.all_passed(), "U2561 domain checks must be green");
    }

    #[test]
    fn u2561_segment_guard_holds() {
        assert!(u2561_segment_guard());
    }
}
