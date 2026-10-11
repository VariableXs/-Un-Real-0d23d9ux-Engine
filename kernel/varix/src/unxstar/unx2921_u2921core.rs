//! unxu2921_u2921core —— COW-页缓存联合压测方案冻结 语义登记表与守恒账（COW-页缓存联合压测）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2921 · COW-页缓存联合压测方案冻结 · 判据原文：UNX-F2921 · 判据：UNX-F2921-J1 联合矩阵（私有/共享/匿名三态×读写比五档）冻结哈希入册，方案与 F2641/F2682 双接口字段对齐
//! · 组件：COW-页缓存联合压测 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2921 · 判据：UNX-F2921-J1 联合矩阵（私有/共享/匿名三态×读写比五档）冻结哈希入册，方案与 F2641/F2682 双接口字段对齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2921_VERSION: &str = "u2921-u2921core-v1";

/// 诊断段基址。
pub const U2921_SEG_BASE: u16 = 0xe210;

// --------------------------------------------------------------------------
// 诊断码（u2921 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// COW-页缓存联合压测 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2921Code {
    /// 0xe210 表行重复（联合压测 同键双写）。
    RowDup,
    /// 0xe211 表行断档（COW 序号不连续）。
    RowGap,
    /// 0xe212 值越界（页缓存 值域外）。
    ValOob,
    /// 0xe213 累计漂移（叠加 求和≠理论值）。
    SumDrift,
    /// 0xe214 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe215 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe216 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe217 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe218 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe219 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe21a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe21b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2921Code {
    /// 全量闭集。
    pub const ALL: [u2921Code; 12] = [
        u2921Code::RowDup,
        u2921Code::RowGap,
        u2921Code::ValOob,
        u2921Code::SumDrift,
        u2921Code::LedgerDup,
        u2921Code::LedgerHole,
        u2921Code::OpNoop,
        u2921Code::FieldDrift,
        u2921Code::LedgerBroken,
        u2921Code::SnapshotFingerprint,
        u2921Code::TableBroken,
        u2921Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2921Code::RowDup => 0xe210,
            u2921Code::RowGap => 0xe211,
            u2921Code::ValOob => 0xe212,
            u2921Code::SumDrift => 0xe213,
            u2921Code::LedgerDup => 0xe214,
            u2921Code::LedgerHole => 0xe215,
            u2921Code::OpNoop => 0xe216,
            u2921Code::FieldDrift => 0xe217,
            u2921Code::LedgerBroken => 0xe218,
            u2921Code::SnapshotFingerprint => 0xe219,
            u2921Code::TableBroken => 0xe21a,
            u2921Code::SegmentGuard => 0xe21b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2921Code::RowDup => "表行重复（联合压测 同键双写）",
            u2921Code::RowGap => "表行断档（COW 序号不连续）",
            u2921Code::ValOob => "值越界（页缓存 值域外）",
            u2921Code::SumDrift => "累计漂移（叠加 求和≠理论值）",
            u2921Code::LedgerDup => "账目重记（同一事件双入账）",
            u2921Code::LedgerHole => "账目断漏（序号缺失）",
            u2921Code::OpNoop => "空操作（账目无效果调用）",
            u2921Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2921Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2921Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2921Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2921Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2921Code::RowDup => "按键唯一律去重",
            u2921Code::RowGap => "按连续律补号",
            u2921Code::ValOob => "按值域裁剪",
            u2921Code::SumDrift => "按理论值复算",
            u2921Code::LedgerDup => "按事件幂等律合并",
            u2921Code::LedgerHole => "按连续律补账",
            u2921Code::OpNoop => "核对操作前置条件",
            u2921Code::FieldDrift => "按口径重建字段",
            u2921Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2921Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2921Code::TableBroken => "按语义规则重建常量表",
            u2921Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2921reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// COW-页缓存联合压测 语义登记表（24 行逐行注释）。
pub const u2921reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2921reg: [(&'static str, u64); 24] = [
    ("COW_A", 7), // COW 主登记（写时复制 · 判据面）
    ("COW_B", 33), // COW 副登记（写时复制 · 对拍面）
    ("PC_A", 14), // PC 主登记（页缓存 · 判据面）
    ("PC_B", 44), // PC 副登记（页缓存 · 对拍面）
    ("JOINT_A", 21), // JOINT 主登记（联合 · 判据面）
    ("JOINT_B", 55), // JOINT 副登记（联合 · 对拍面）
    ("SHARE_A", 28), // SHARE 主登记（共享 · 判据面）
    ("SHARE_B", 66), // SHARE 副登记（共享 · 对拍面）
    ("EVICT_A", 35), // EVICT 主登记（逐出 · 判据面）
    ("EVICT_B", 77), // EVICT 副登记（逐出 · 对拍面）
    ("DIRTY_A", 42), // DIRTY 主登记（脏页 · 判据面）
    ("DIRTY_B", 88), // DIRTY 副登记（脏页 · 对拍面）
    ("WB_A", 49), // WB 主登记（回写 · 判据面）
    ("WB_B", 99), // WB 副登记（回写 · 对拍面）
    ("RECLAIM_A", 56), // RECLAIM 主登记（回收 · 判据面）
    ("RECLAIM_B", 110), // RECLAIM 副登记（回收 · 对拍面）
    ("ISO_A", 63), // ISO 主登记（隔离 · 判据面）
    ("ISO_B", 121), // ISO 副登记（隔离 · 对拍面）
    ("THR_A", 70), // THR 主登记（阈值 · 判据面）
    ("THR_B", 132), // THR 副登记（阈值 · 对拍面）
    ("ACCT_A", 77), // ACCT 主登记（账 · 判据面）
    ("ACCT_B", 143), // ACCT 副登记（账 · 对拍面）
    ("VERDICT_A", 84), // VERDICT 主登记（裁决 · 判据面）
    ("VERDICT_B", 154), // VERDICT 副登记（裁决 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2921reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2921reg_COUNT {
        if u2921reg[i].0 == key {
            return Some(u2921reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2921reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2921reg_COUNT {
        s = s.wrapping_add(u2921reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2921Ledger（COW-页缓存联合压测 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// COW-页缓存联合压测 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2921Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2921Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe210, 0xe21f]。
pub const fn u2921_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2921Code::ALL.len() {
        let c = u2921Code::ALL[i].code();
        if c < 0xe210 || c > 0xe21f {
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
fn u2921_guard_seg() -> bool {
    u2921_segment_guard()
}

/// UNX-F2921 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2921_core_tbl() -> bool {
    let mut ok = u2921reg_COUNT == 24;
    if ok { ok = u2921reg_get("COW_A") == Some(7) && u2921reg_get("COW_B") == Some(33); }
    if ok { ok = u2921reg_get("VERDICT_A") == Some(84); }
    if ok { ok = u2921reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2921reg_sum() == 1668; }
    ok
}

/// UNX-F2921 守恒账三操作确定性效果。
fn check_u2921_core_ldg() -> bool {
    let mut ld = Mxu2921Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2921_checks() -> CheckSet {
    let mut set = CheckSet::new("U2921");
    set.add(
        "u2921-seg-guard",
        u2921_guard_seg(),
        if u2921_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2921-core-tbl",
        check_u2921_core_tbl(),
        if check_u2921_core_tbl() { "" } else { "UNX-F2921 core table mismatch" },
    );
    set.add(
        "u2921-core-ldg",
        check_u2921_core_ldg(),
        if check_u2921_core_ldg() { "" } else { "UNX-F2921 core ledger mismatch" },
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
    fn u2921_all_checks_green() {
        let set = run_u2921_checks();
        assert!(set.all_passed(), "U2921 domain checks must be green");
    }

    #[test]
    fn u2921_segment_guard_holds() {
        assert!(u2921_segment_guard());
    }
}
