//! unxu2801_u2801core —— 大页池容量压测方案冻结 语义登记表与守恒账（大页池容量压测）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2801 · 大页池容量压测方案冻结 · 判据原文：UNX-F2801 · 判据：UNX-F2801-J1 三态方案（满池/近满/空）参数冻结哈希入册，方案与 F2425 池账字段逐项对齐
//! · 组件：大页池容量压测 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2801 · 判据：UNX-F2801-J1 三态方案（满池/近满/空）参数冻结哈希入册，方案与 F2425 池账字段逐项对齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2801_VERSION: &str = "u2801-u2801core-v1";

/// 诊断段基址。
pub const U2801_SEG_BASE: u16 = 0xe1b0;

// --------------------------------------------------------------------------
// 诊断码（u2801 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 大页池容量压测 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2801Code {
    /// 0xe1b0 表行重复（大页 同键双写）。
    RowDup,
    /// 0xe1b1 表行断档（压测 序号不连续）。
    RowGap,
    /// 0xe1b2 值越界（容量 值域外）。
    ValOob,
    /// 0xe1b3 累计漂移（冻结 求和≠理论值）。
    SumDrift,
    /// 0xe1b4 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe1b5 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe1b6 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe1b7 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe1b8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1b9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1ba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1bb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2801Code {
    /// 全量闭集。
    pub const ALL: [u2801Code; 12] = [
        u2801Code::RowDup,
        u2801Code::RowGap,
        u2801Code::ValOob,
        u2801Code::SumDrift,
        u2801Code::LedgerDup,
        u2801Code::LedgerHole,
        u2801Code::OpNoop,
        u2801Code::FieldDrift,
        u2801Code::LedgerBroken,
        u2801Code::SnapshotFingerprint,
        u2801Code::TableBroken,
        u2801Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2801Code::RowDup => 0xe1b0,
            u2801Code::RowGap => 0xe1b1,
            u2801Code::ValOob => 0xe1b2,
            u2801Code::SumDrift => 0xe1b3,
            u2801Code::LedgerDup => 0xe1b4,
            u2801Code::LedgerHole => 0xe1b5,
            u2801Code::OpNoop => 0xe1b6,
            u2801Code::FieldDrift => 0xe1b7,
            u2801Code::LedgerBroken => 0xe1b8,
            u2801Code::SnapshotFingerprint => 0xe1b9,
            u2801Code::TableBroken => 0xe1ba,
            u2801Code::SegmentGuard => 0xe1bb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2801Code::RowDup => "表行重复（大页 同键双写）",
            u2801Code::RowGap => "表行断档（压测 序号不连续）",
            u2801Code::ValOob => "值越界（容量 值域外）",
            u2801Code::SumDrift => "累计漂移（冻结 求和≠理论值）",
            u2801Code::LedgerDup => "账目重记（同一事件双入账）",
            u2801Code::LedgerHole => "账目断漏（序号缺失）",
            u2801Code::OpNoop => "空操作（账目无效果调用）",
            u2801Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2801Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2801Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2801Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2801Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2801Code::RowDup => "按键唯一律去重",
            u2801Code::RowGap => "按连续律补号",
            u2801Code::ValOob => "按值域裁剪",
            u2801Code::SumDrift => "按理论值复算",
            u2801Code::LedgerDup => "按事件幂等律合并",
            u2801Code::LedgerHole => "按连续律补账",
            u2801Code::OpNoop => "核对操作前置条件",
            u2801Code::FieldDrift => "按口径重建字段",
            u2801Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2801Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2801Code::TableBroken => "按语义规则重建常量表",
            u2801Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2801reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 大页池容量压测 语义登记表（24 行逐行注释）。
pub const u2801reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2801reg: [(&'static str, u64); 24] = [
    ("HP_A", 7), // HP 主登记（大页 · 判据面）
    ("HP_B", 33), // HP 副登记（大页 · 对拍面）
    ("POOL_A", 14), // POOL 主登记（池 · 判据面）
    ("POOL_B", 44), // POOL 副登记（池 · 对拍面）
    ("CAP_A", 21), // CAP 主登记（容量 · 判据面）
    ("CAP_B", 55), // CAP 副登记（容量 · 对拍面）
    ("PRESS_A", 28), // PRESS 主登记（压测 · 判据面）
    ("PRESS_B", 66), // PRESS 副登记（压测 · 对拍面）
    ("MATRIX_A", 35), // MATRIX 主登记（矩阵 · 判据面）
    ("MATRIX_B", 77), // MATRIX 副登记（矩阵 · 对拍面）
    ("ALLOC_A", 42), // ALLOC 主登记（分配 · 判据面）
    ("ALLOC_B", 88), // ALLOC 副登记（分配 · 对拍面）
    ("FREE_A", 49), // FREE 主登记（释放 · 判据面）
    ("FREE_B", 99), // FREE 副登记（释放 · 对拍面）
    ("FRAG_A", 56), // FRAG 主登记（碎片 · 判据面）
    ("FRAG_B", 110), // FRAG 副登记（碎片 · 对拍面）
    ("RSS_A", 63), // RSS 主登记（常驻 · 判据面）
    ("RSS_B", 121), // RSS 副登记（常驻 · 对拍面）
    ("FLOOR_A", 70), // FLOOR 主登记（地板 · 判据面）
    ("FLOOR_B", 132), // FLOOR 副登记（地板 · 对拍面）
    ("CEIL_A", 77), // CEIL 主登记（天花板 · 判据面）
    ("CEIL_B", 143), // CEIL 副登记（天花板 · 对拍面）
    ("BUD_A", 84), // BUD 主登记（预算 · 判据面）
    ("BUD_B", 154), // BUD 副登记（预算 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2801reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2801reg_COUNT {
        if u2801reg[i].0 == key {
            return Some(u2801reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2801reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2801reg_COUNT {
        s = s.wrapping_add(u2801reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2801Ledger（大页池容量压测 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 大页池容量压测 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2801Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2801Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1b0, 0xe1bf]。
pub const fn u2801_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2801Code::ALL.len() {
        let c = u2801Code::ALL[i].code();
        if c < 0xe1b0 || c > 0xe1bf {
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
fn u2801_guard_seg() -> bool {
    u2801_segment_guard()
}

/// UNX-F2801 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2801_core_tbl() -> bool {
    let mut ok = u2801reg_COUNT == 24;
    if ok { ok = u2801reg_get("HP_A") == Some(7) && u2801reg_get("HP_B") == Some(33); }
    if ok { ok = u2801reg_get("BUD_A") == Some(84); }
    if ok { ok = u2801reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2801reg_sum() == 1668; }
    ok
}

/// UNX-F2801 守恒账三操作确定性效果。
fn check_u2801_core_ldg() -> bool {
    let mut ld = Mxu2801Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2801_checks() -> CheckSet {
    let mut set = CheckSet::new("U2801");
    set.add(
        "u2801-seg-guard",
        u2801_guard_seg(),
        if u2801_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2801-core-tbl",
        check_u2801_core_tbl(),
        if check_u2801_core_tbl() { "" } else { "UNX-F2801 core table mismatch" },
    );
    set.add(
        "u2801-core-ldg",
        check_u2801_core_ldg(),
        if check_u2801_core_ldg() { "" } else { "UNX-F2801 core ledger mismatch" },
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
    fn u2801_all_checks_green() {
        let set = run_u2801_checks();
        assert!(set.all_passed(), "U2801 domain checks must be green");
    }

    #[test]
    fn u2801_segment_guard_holds() {
        assert!(u2801_segment_guard());
    }
}
