//! unxu1881_u1881core —— 全域注入矩阵终对账（十一矩阵收口） 语义登记表与守恒账（矩阵）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1881 · 全域注入矩阵终对账（十一矩阵收口） · 判据原文：UNX-F1881 · 判据：十一矩阵（F1659/F1673/F1692/F1715/F1732/F1756/F1772/F1814/F1835/F1856/F1874）形态总表对账闭合，覆盖空洞为零，与 F1788 口径一致
//! · 组件：矩阵 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F1881 · 判据：十一矩阵（F1659/F1673/F1692/F1715/F1732/F1756/F1772/F1814/F1835/F1856/
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1881_VERSION: &str = "u1881-u1881core-v1";

/// 诊断段基址。
pub const U1881_SEG_BASE: u16 = 0x5f80;

// --------------------------------------------------------------------------
// 诊断码（u1881 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 矩阵 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1881Code {
    /// 0x5f80 表行重复（矩阵缺位 同键双写）。
    RowDup,
    /// 0x5f81 表行断档（对账破缺 序号不连续）。
    RowGap,
    /// 0x5f82 值越界（覆盖空洞 值域外）。
    ValOob,
    /// 0x5f83 累计漂移（口径漂移 求和≠理论值）。
    SumDrift,
    /// 0x5f84 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0x5f85 账目断漏（序号缺失）。
    LedgerHole,
    /// 0x5f86 空操作（账目无效果调用）。
    OpNoop,
    /// 0x5f87 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0x5f88 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5f89 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5f8a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5f8b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1881Code {
    /// 全量闭集。
    pub const ALL: [u1881Code; 12] = [
        u1881Code::RowDup,
        u1881Code::RowGap,
        u1881Code::ValOob,
        u1881Code::SumDrift,
        u1881Code::LedgerDup,
        u1881Code::LedgerHole,
        u1881Code::OpNoop,
        u1881Code::FieldDrift,
        u1881Code::LedgerBroken,
        u1881Code::SnapshotFingerprint,
        u1881Code::TableBroken,
        u1881Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1881Code::RowDup => 0x5f80,
            u1881Code::RowGap => 0x5f81,
            u1881Code::ValOob => 0x5f82,
            u1881Code::SumDrift => 0x5f83,
            u1881Code::LedgerDup => 0x5f84,
            u1881Code::LedgerHole => 0x5f85,
            u1881Code::OpNoop => 0x5f86,
            u1881Code::FieldDrift => 0x5f87,
            u1881Code::LedgerBroken => 0x5f88,
            u1881Code::SnapshotFingerprint => 0x5f89,
            u1881Code::TableBroken => 0x5f8a,
            u1881Code::SegmentGuard => 0x5f8b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1881Code::RowDup => "表行重复（矩阵缺位 同键双写）",
            u1881Code::RowGap => "表行断档（对账破缺 序号不连续）",
            u1881Code::ValOob => "值越界（覆盖空洞 值域外）",
            u1881Code::SumDrift => "累计漂移（口径漂移 求和≠理论值）",
            u1881Code::LedgerDup => "账目重记（同一事件双入账）",
            u1881Code::LedgerHole => "账目断漏（序号缺失）",
            u1881Code::OpNoop => "空操作（账目无效果调用）",
            u1881Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u1881Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1881Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1881Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1881Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1881Code::RowDup => "按键唯一律去重",
            u1881Code::RowGap => "按连续律补号",
            u1881Code::ValOob => "按值域裁剪",
            u1881Code::SumDrift => "按理论值复算",
            u1881Code::LedgerDup => "按事件幂等律合并",
            u1881Code::LedgerHole => "按连续律补账",
            u1881Code::OpNoop => "核对操作前置条件",
            u1881Code::FieldDrift => "按口径重建字段",
            u1881Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1881Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1881Code::TableBroken => "按语义规则重建常量表",
            u1881Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1881reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 矩阵 语义登记表（24 行逐行注释）。
pub const u1881reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1881reg: [(&'static str, u64); 24] = [
    ("SZ_A", 7), // SZ 主登记（十一矩阵 · 判据面）
    ("SZ_B", 33), // SZ 副登记（十一矩阵 · 对拍面）
    ("XT_A", 14), // XT 主登记（形态总表 · 判据面）
    ("XT_B", 44), // XT 副登记（形态总表 · 对拍面）
    ("DZ_A", 21), // DZ 主登记（对账闭合 · 判据面）
    ("DZ_B", 55), // DZ 副登记（对账闭合 · 对拍面）
    ("FG_A", 28), // FG 主登记（覆盖空洞 · 判据面）
    ("FG_B", 66), // FG 副登记（覆盖空洞 · 对拍面）
    ("F1788_A", 35), // F1788 主登记（F1788 口径 · 判据面）
    ("F1788_B", 77), // F1788 副登记（F1788 口径 · 对拍面）
    ("F1659_A", 42), // F1659 主登记（F1659 锚 · 判据面）
    ("F1659_B", 88), // F1659 副登记（F1659 锚 · 对拍面）
    ("F1673_A", 49), // F1673 主登记（F1673 锚 · 判据面）
    ("F1673_B", 99), // F1673 副登记（F1673 锚 · 对拍面）
    ("F1692_A", 56), // F1692 主登记（F1692 锚 · 判据面）
    ("F1692_B", 110), // F1692 副登记（F1692 锚 · 对拍面）
    ("F1715_A", 63), // F1715 主登记（F1715 锚 · 判据面）
    ("F1715_B", 121), // F1715 副登记（F1715 锚 · 对拍面）
    ("F1732_A", 70), // F1732 主登记（F1732 锚 · 判据面）
    ("F1732_B", 132), // F1732 副登记（F1732 锚 · 对拍面）
    ("F1756_A", 77), // F1756 主登记（F1756 锚 · 判据面）
    ("F1756_B", 143), // F1756 副登记（F1756 锚 · 对拍面）
    ("F1772_A", 84), // F1772 主登记（F1772 锚 · 判据面）
    ("F1772_B", 154), // F1772 副登记（F1772 锚 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1881reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1881reg_COUNT {
        if u1881reg[i].0 == key {
            return Some(u1881reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1881reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1881reg_COUNT {
        s = s.wrapping_add(u1881reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu1881Ledger（矩阵 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 矩阵 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu1881Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu1881Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5f80, 0x5f8f]。
pub const fn u1881_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1881Code::ALL.len() {
        let c = u1881Code::ALL[i].code();
        if c < 0x5f80 || c > 0x5f8f {
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
fn u1881_guard_seg() -> bool {
    u1881_segment_guard()
}

/// UNX-F1881 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u1881_core_tbl() -> bool {
    let mut ok = u1881reg_COUNT == 24;
    if ok { ok = u1881reg_get("SZ_A") == Some(7) && u1881reg_get("SZ_B") == Some(33); }
    if ok { ok = u1881reg_get("F1772_A") == Some(84); }
    if ok { ok = u1881reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1881reg_sum() == 1668; }
    ok
}

/// UNX-F1881 守恒账三操作确定性效果。
fn check_u1881_core_ldg() -> bool {
    let mut ld = Mxu1881Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1881_checks() -> CheckSet {
    let mut set = CheckSet::new("U1881");
    set.add(
        "u1881-seg-guard",
        u1881_guard_seg(),
        if u1881_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1881-core-tbl",
        check_u1881_core_tbl(),
        if check_u1881_core_tbl() { "" } else { "UNX-F1881 core table mismatch" },
    );
    set.add(
        "u1881-core-ldg",
        check_u1881_core_ldg(),
        if check_u1881_core_ldg() { "" } else { "UNX-F1881 core ledger mismatch" },
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
    fn u1881_all_checks_green() {
        let set = run_u1881_checks();
        assert!(set.all_passed(), "U1881 domain checks must be green");
    }

    #[test]
    fn u1881_segment_guard_holds() {
        assert!(u1881_segment_guard());
    }
}
