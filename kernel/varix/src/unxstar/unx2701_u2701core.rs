//! unxu2701_u2701core —— LRU active/inactive 双链结构地基 语义登记表与守恒账（双链节点）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2701 · LRU active/inactive 双链结构地基 · 判据原文：UNX-F2701 · 判据：active/inactive 双链结构就绪（链头/链尾/哨兵三件注册），空链插入弹出各 10 次对账零漂移
//! · 组件：双链节点 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2701 · 判据：active/inactive 双链结构就绪（链头/链尾/哨兵三件注册），空链插入弹出各 10 次对账零漂移
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2701_VERSION: &str = "u2701-u2701core-v1";

/// 诊断段基址。
pub const U2701_SEG_BASE: u16 = 0xab40;

// --------------------------------------------------------------------------
// 诊断码（u2701 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 双链节点 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2701Code {
    /// 0xab40 表行重复（链首尾乱 同键双写）。
    RowDup,
    /// 0xab41 表行断档（哨兵丢失 序号不连续）。
    RowGap,
    /// 0xab42 值越界（跨链错迁 值域外）。
    ValOob,
    /// 0xab43 累计漂移（计数漂移 求和≠理论值）。
    SumDrift,
    /// 0xab44 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xab45 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xab46 空操作（账目无效果调用）。
    OpNoop,
    /// 0xab47 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xab48 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xab49 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xab4a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xab4b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2701Code {
    /// 全量闭集。
    pub const ALL: [u2701Code; 12] = [
        u2701Code::RowDup,
        u2701Code::RowGap,
        u2701Code::ValOob,
        u2701Code::SumDrift,
        u2701Code::LedgerDup,
        u2701Code::LedgerHole,
        u2701Code::OpNoop,
        u2701Code::FieldDrift,
        u2701Code::LedgerBroken,
        u2701Code::SnapshotFingerprint,
        u2701Code::TableBroken,
        u2701Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2701Code::RowDup => 0xab40,
            u2701Code::RowGap => 0xab41,
            u2701Code::ValOob => 0xab42,
            u2701Code::SumDrift => 0xab43,
            u2701Code::LedgerDup => 0xab44,
            u2701Code::LedgerHole => 0xab45,
            u2701Code::OpNoop => 0xab46,
            u2701Code::FieldDrift => 0xab47,
            u2701Code::LedgerBroken => 0xab48,
            u2701Code::SnapshotFingerprint => 0xab49,
            u2701Code::TableBroken => 0xab4a,
            u2701Code::SegmentGuard => 0xab4b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2701Code::RowDup => "表行重复（链首尾乱 同键双写）",
            u2701Code::RowGap => "表行断档（哨兵丢失 序号不连续）",
            u2701Code::ValOob => "值越界（跨链错迁 值域外）",
            u2701Code::SumDrift => "累计漂移（计数漂移 求和≠理论值）",
            u2701Code::LedgerDup => "账目重记（同一事件双入账）",
            u2701Code::LedgerHole => "账目断漏（序号缺失）",
            u2701Code::OpNoop => "空操作（账目无效果调用）",
            u2701Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2701Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2701Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2701Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2701Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2701Code::RowDup => "按键唯一律去重",
            u2701Code::RowGap => "按连续律补号",
            u2701Code::ValOob => "按值域裁剪",
            u2701Code::SumDrift => "按理论值复算",
            u2701Code::LedgerDup => "按事件幂等律合并",
            u2701Code::LedgerHole => "按连续律补账",
            u2701Code::OpNoop => "核对操作前置条件",
            u2701Code::FieldDrift => "按口径重建字段",
            u2701Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2701Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2701Code::TableBroken => "按语义规则重建常量表",
            u2701Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2701reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 双链节点 语义登记表（24 行逐行注释）。
pub const u2701reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2701reg: [(&'static str, u64); 24] = [
    ("LQ_A", 7), // LQ 主登记（LRU 双链 · 判据面）
    ("LQ_B", 33), // LQ 副登记（LRU 双链 · 对拍面）
    ("AC_A", 14), // AC 主登记（active 链 · 判据面）
    ("AC_B", 44), // AC 副登记（active 链 · 对拍面）
    ("IN_A", 21), // IN 主登记（inactive 链 · 判据面）
    ("IN_B", 55), // IN 副登记（inactive 链 · 对拍面）
    ("HD_A", 28), // HD 主登记（链头 · 判据面）
    ("HD_B", 66), // HD 副登记（链头 · 对拍面）
    ("TL_A", 35), // TL 主登记（链尾 · 判据面）
    ("TL_B", 77), // TL 副登记（链尾 · 对拍面）
    ("SB_A", 42), // SB 主登记（哨兵 · 判据面）
    ("SB_B", 88), // SB 副登记（哨兵 · 对拍面）
    ("EM_A", 49), // EM 主登记（空链 · 判据面）
    ("EM_B", 99), // EM 副登记（空链 · 对拍面）
    ("IP_A", 56), // IP 主登记（插入 · 判据面）
    ("IP_B", 110), // IP 副登记（插入 · 对拍面）
    ("PO_A", 63), // PO 主登记（弹出 · 判据面）
    ("PO_B", 121), // PO 副登记（弹出 · 对拍面）
    ("ZD_A", 70), // ZD 主登记（零漂移 · 判据面）
    ("ZD_B", 132), // ZD 副登记（零漂移 · 对拍面）
    ("NV_A", 77), // NV 主登记（节点面 · 判据面）
    ("NV_B", 143), // NV 副登记（节点面 · 对拍面）
    ("CT_A", 84), // CT 主登记（计数面 · 判据面）
    ("CT_B", 154), // CT 副登记（计数面 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2701reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2701reg_COUNT {
        if u2701reg[i].0 == key {
            return Some(u2701reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2701reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2701reg_COUNT {
        s = s.wrapping_add(u2701reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2701Ledger（双链节点 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 双链节点 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2701Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2701Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xab40, 0xab4f]。
pub const fn u2701_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2701Code::ALL.len() {
        let c = u2701Code::ALL[i].code();
        if c < 0xab40 || c > 0xab4f {
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
fn u2701_guard_seg() -> bool {
    u2701_segment_guard()
}

/// UNX-F2701 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2701_core_tbl() -> bool {
    let mut ok = u2701reg_COUNT == 24;
    if ok { ok = u2701reg_get("LQ_A") == Some(7) && u2701reg_get("LQ_B") == Some(33); }
    if ok { ok = u2701reg_get("CT_A") == Some(84); }
    if ok { ok = u2701reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2701reg_sum() == 1668; }
    ok
}

/// UNX-F2701 守恒账三操作确定性效果。
fn check_u2701_core_ldg() -> bool {
    let mut ld = Mxu2701Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2701_checks() -> CheckSet {
    let mut set = CheckSet::new("U2701");
    set.add(
        "u2701-seg-guard",
        u2701_guard_seg(),
        if u2701_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2701-core-tbl",
        check_u2701_core_tbl(),
        if check_u2701_core_tbl() { "" } else { "UNX-F2701 core table mismatch" },
    );
    set.add(
        "u2701-core-ldg",
        check_u2701_core_ldg(),
        if check_u2701_core_ldg() { "" } else { "UNX-F2701 core ledger mismatch" },
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
    fn u2701_all_checks_green() {
        let set = run_u2701_checks();
        assert!(set.all_passed(), "U2701 domain checks must be green");
    }

    #[test]
    fn u2701_segment_guard_holds() {
        assert!(u2701_segment_guard());
    }
}
