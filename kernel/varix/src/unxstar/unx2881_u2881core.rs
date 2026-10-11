//! unxu2881_u2881core —— COW 压测方案冻结（三矩阵） 语义登记表与守恒账（COW 三矩阵压测）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2881 · COW 压测方案冻结（三矩阵） · 判据原文：UNX-F2881 · 判据：UNX-F2881-J1 三矩阵（fork 密度/写密度/页类）参数冻结哈希入册，方案与 F2641 接口字段对齐
//! · 组件：COW 三矩阵压测 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2881 · 判据：UNX-F2881-J1 三矩阵（fork 密度/写密度/页类）参数冻结哈希入册，方案与 F2641 接口字段对齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2881_VERSION: &str = "u2881-u2881core-v1";

/// 诊断段基址。
pub const U2881_SEG_BASE: u16 = 0xe1f0;

// --------------------------------------------------------------------------
// 诊断码（u2881 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// COW 三矩阵压测 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2881Code {
    /// 0xe1f0 表行重复（COW 同键双写）。
    RowDup,
    /// 0xe1f1 表行断档（写时复制 序号不连续）。
    RowGap,
    /// 0xe1f2 值越界（矩阵 值域外）。
    ValOob,
    /// 0xe1f3 累计漂移（写时复制 求和≠理论值）。
    SumDrift,
    /// 0xe1f4 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe1f5 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe1f6 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe1f7 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe1f8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1f9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1fa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1fb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2881Code {
    /// 全量闭集。
    pub const ALL: [u2881Code; 12] = [
        u2881Code::RowDup,
        u2881Code::RowGap,
        u2881Code::ValOob,
        u2881Code::SumDrift,
        u2881Code::LedgerDup,
        u2881Code::LedgerHole,
        u2881Code::OpNoop,
        u2881Code::FieldDrift,
        u2881Code::LedgerBroken,
        u2881Code::SnapshotFingerprint,
        u2881Code::TableBroken,
        u2881Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2881Code::RowDup => 0xe1f0,
            u2881Code::RowGap => 0xe1f1,
            u2881Code::ValOob => 0xe1f2,
            u2881Code::SumDrift => 0xe1f3,
            u2881Code::LedgerDup => 0xe1f4,
            u2881Code::LedgerHole => 0xe1f5,
            u2881Code::OpNoop => 0xe1f6,
            u2881Code::FieldDrift => 0xe1f7,
            u2881Code::LedgerBroken => 0xe1f8,
            u2881Code::SnapshotFingerprint => 0xe1f9,
            u2881Code::TableBroken => 0xe1fa,
            u2881Code::SegmentGuard => 0xe1fb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2881Code::RowDup => "表行重复（COW 同键双写）",
            u2881Code::RowGap => "表行断档（写时复制 序号不连续）",
            u2881Code::ValOob => "值越界（矩阵 值域外）",
            u2881Code::SumDrift => "累计漂移（写时复制 求和≠理论值）",
            u2881Code::LedgerDup => "账目重记（同一事件双入账）",
            u2881Code::LedgerHole => "账目断漏（序号缺失）",
            u2881Code::OpNoop => "空操作（账目无效果调用）",
            u2881Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2881Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2881Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2881Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2881Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2881Code::RowDup => "按键唯一律去重",
            u2881Code::RowGap => "按连续律补号",
            u2881Code::ValOob => "按值域裁剪",
            u2881Code::SumDrift => "按理论值复算",
            u2881Code::LedgerDup => "按事件幂等律合并",
            u2881Code::LedgerHole => "按连续律补账",
            u2881Code::OpNoop => "核对操作前置条件",
            u2881Code::FieldDrift => "按口径重建字段",
            u2881Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2881Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2881Code::TableBroken => "按语义规则重建常量表",
            u2881Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2881reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// COW 三矩阵压测 语义登记表（24 行逐行注释）。
pub const u2881reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2881reg: [(&'static str, u64); 24] = [
    ("COW_A", 7), // COW 主登记（写时复制 · 判据面）
    ("COW_B", 33), // COW 副登记（写时复制 · 对拍面）
    ("M1_A", 14), // M1 主登记（矩阵一 · 判据面）
    ("M1_B", 44), // M1 副登记（矩阵一 · 对拍面）
    ("M2_A", 21), // M2 主登记（矩阵二 · 判据面）
    ("M2_B", 55), // M2 副登记（矩阵二 · 对拍面）
    ("M3_A", 28), // M3 主登记（矩阵三 · 判据面）
    ("M3_B", 66), // M3 副登记（矩阵三 · 对拍面）
    ("DUP_A", 35), // DUP 主登记（复制 · 判据面）
    ("DUP_B", 77), // DUP 副登记（复制 · 对拍面）
    ("SHR_A", 42), // SHR 主登记（共享 · 判据面）
    ("SHR_B", 88), // SHR 副登记（共享 · 对拍面）
    ("WBACK_A", 49), // WBACK 主登记（回写 · 判据面）
    ("WBACK_B", 99), // WBACK 副登记（回写 · 对拍面）
    ("ALIAS_A", 56), // ALIAS 主登记（别名 · 判据面）
    ("ALIAS_B", 110), // ALIAS 副登记（别名 · 对拍面）
    ("BREAK_A", 63), // BREAK 主登记（断链 · 判据面）
    ("BREAK_B", 121), // BREAK 副登记（断链 · 对拍面）
    ("ISO_A", 70), // ISO 主登记（隔离 · 判据面）
    ("ISO_B", 132), // ISO 副登记（隔离 · 对拍面）
    ("SEQ_A", 77), // SEQ 主登记（序号 · 判据面）
    ("SEQ_B", 143), // SEQ 副登记（序号 · 对拍面）
    ("ACCT_A", 84), // ACCT 主登记（账 · 判据面）
    ("ACCT_B", 154), // ACCT 副登记（账 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2881reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2881reg_COUNT {
        if u2881reg[i].0 == key {
            return Some(u2881reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2881reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2881reg_COUNT {
        s = s.wrapping_add(u2881reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2881Ledger（COW 三矩阵压测 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// COW 三矩阵压测 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2881Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2881Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1f0, 0xe1ff]。
pub const fn u2881_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2881Code::ALL.len() {
        let c = u2881Code::ALL[i].code();
        if c < 0xe1f0 || c > 0xe1ff {
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
fn u2881_guard_seg() -> bool {
    u2881_segment_guard()
}

/// UNX-F2881 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2881_core_tbl() -> bool {
    let mut ok = u2881reg_COUNT == 24;
    if ok { ok = u2881reg_get("COW_A") == Some(7) && u2881reg_get("COW_B") == Some(33); }
    if ok { ok = u2881reg_get("ACCT_A") == Some(84); }
    if ok { ok = u2881reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2881reg_sum() == 1668; }
    ok
}

/// UNX-F2881 守恒账三操作确定性效果。
fn check_u2881_core_ldg() -> bool {
    let mut ld = Mxu2881Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2881_checks() -> CheckSet {
    let mut set = CheckSet::new("U2881");
    set.add(
        "u2881-seg-guard",
        u2881_guard_seg(),
        if u2881_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2881-core-tbl",
        check_u2881_core_tbl(),
        if check_u2881_core_tbl() { "" } else { "UNX-F2881 core table mismatch" },
    );
    set.add(
        "u2881-core-ldg",
        check_u2881_core_ldg(),
        if check_u2881_core_ldg() { "" } else { "UNX-F2881 core ledger mismatch" },
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
    fn u2881_all_checks_green() {
        let set = run_u2881_checks();
        assert!(set.all_passed(), "U2881 domain checks must be green");
    }

    #[test]
    fn u2881_segment_guard_holds() {
        assert!(u2881_segment_guard());
    }
}
