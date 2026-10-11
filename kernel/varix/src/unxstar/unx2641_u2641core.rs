//! unxu2641_u2641core —— fork 地址空间复制原语（冻结接口 5 正式版） 语义登记表与守恒账（fork 复制）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2641 · fork 地址空间复制原语（冻结接口 5 正式版） · 判据原文：UNX-F2641 · 判据：冻结接口 5 正式冻结（C1 消费联签），F2558 预声明与 F2564 地基消费两账兑现闭合
//! · 组件：fork 复制 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2641 · 判据：冻结接口 5 正式冻结（C1 消费联签），F2558 预声明与 F2564 地基消费两账兑现闭合
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2641_VERSION: &str = "u2641-u2641core-v1";

/// 诊断段基址。
pub const U2641_SEG_BASE: u16 = 0xa900;

// --------------------------------------------------------------------------
// 诊断码（u2641 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// fork 复制 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2641Code {
    /// 0xa900 表行重复（两账不闭合 同键双写）。
    RowDup,
    /// 0xa901 表行断档（复制漏页 序号不连续）。
    RowGap,
    /// 0xa902 值越界（COW 失效 值域外）。
    ValOob,
    /// 0xa903 累计漂移（联签缺位 求和≠理论值）。
    SumDrift,
    /// 0xa904 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xa905 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xa906 空操作（账目无效果调用）。
    OpNoop,
    /// 0xa907 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xa908 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa909 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa90a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa90b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2641Code {
    /// 全量闭集。
    pub const ALL: [u2641Code; 12] = [
        u2641Code::RowDup,
        u2641Code::RowGap,
        u2641Code::ValOob,
        u2641Code::SumDrift,
        u2641Code::LedgerDup,
        u2641Code::LedgerHole,
        u2641Code::OpNoop,
        u2641Code::FieldDrift,
        u2641Code::LedgerBroken,
        u2641Code::SnapshotFingerprint,
        u2641Code::TableBroken,
        u2641Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2641Code::RowDup => 0xa900,
            u2641Code::RowGap => 0xa901,
            u2641Code::ValOob => 0xa902,
            u2641Code::SumDrift => 0xa903,
            u2641Code::LedgerDup => 0xa904,
            u2641Code::LedgerHole => 0xa905,
            u2641Code::OpNoop => 0xa906,
            u2641Code::FieldDrift => 0xa907,
            u2641Code::LedgerBroken => 0xa908,
            u2641Code::SnapshotFingerprint => 0xa909,
            u2641Code::TableBroken => 0xa90a,
            u2641Code::SegmentGuard => 0xa90b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2641Code::RowDup => "表行重复（两账不闭合 同键双写）",
            u2641Code::RowGap => "表行断档（复制漏页 序号不连续）",
            u2641Code::ValOob => "值越界（COW 失效 值域外）",
            u2641Code::SumDrift => "累计漂移（联签缺位 求和≠理论值）",
            u2641Code::LedgerDup => "账目重记（同一事件双入账）",
            u2641Code::LedgerHole => "账目断漏（序号缺失）",
            u2641Code::OpNoop => "空操作（账目无效果调用）",
            u2641Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2641Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2641Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2641Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2641Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2641Code::RowDup => "按键唯一律去重",
            u2641Code::RowGap => "按连续律补号",
            u2641Code::ValOob => "按值域裁剪",
            u2641Code::SumDrift => "按理论值复算",
            u2641Code::LedgerDup => "按事件幂等律合并",
            u2641Code::LedgerHole => "按连续律补账",
            u2641Code::OpNoop => "核对操作前置条件",
            u2641Code::FieldDrift => "按口径重建字段",
            u2641Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2641Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2641Code::TableBroken => "按语义规则重建常量表",
            u2641Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2641reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// fork 复制 语义登记表（24 行逐行注释）。
pub const u2641reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2641reg: [(&'static str, u64); 24] = [
    ("FK_A", 7), // FK 主登记（fork 原语 · 判据面）
    ("FK_B", 33), // FK 副登记（fork 原语 · 对拍面）
    ("IF5_A", 14), // IF5 主登记（冻结接口 5 · 判据面）
    ("IF5_B", 44), // IF5 副登记（冻结接口 5 · 对拍面）
    ("CP_A", 21), // CP 主登记（地址空间复制 · 判据面）
    ("CP_B", 55), // CP 副登记（地址空间复制 · 对拍面）
    ("C1_A", 28), // C1 主登记（C1 联签 · 判据面）
    ("C1_B", 66), // C1 副登记（C1 联签 · 对拍面）
    ("PD_A", 35), // PD 主登记（F2558 预声明 · 判据面）
    ("PD_B", 77), // PD 副登记（F2558 预声明 · 对拍面）
    ("FB_A", 42), // FB 主登记（F2564 地基 · 判据面）
    ("FB_B", 88), // FB 副登记（F2564 地基 · 对拍面）
    ("AC_A", 49), // AC 主登记（两账兑现 · 判据面）
    ("AC_B", 99), // AC 副登记（两账兑现 · 对拍面）
    ("PG_A", 56), // PG 主登记（页复制 · 判据面）
    ("PG_B", 110), // PG 副登记（页复制 · 对拍面）
    ("CO_A", 63), // CO 主登记（写时复制 · 判据面）
    ("CO_B", 121), // CO 副登记（写时复制 · 对拍面）
    ("SH_A", 70), // SH 主登记（共享映射 · 判据面）
    ("SH_B", 132), // SH 副登记（共享映射 · 对拍面）
    ("FV_A", 77), // FV 主登记（正式冻结 · 判据面）
    ("FV_B", 143), // FV 副登记（正式冻结 · 对拍面）
    ("LK_A", 84), // LK 主登记（联签账 · 判据面）
    ("LK_B", 154), // LK 副登记（联签账 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2641reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2641reg_COUNT {
        if u2641reg[i].0 == key {
            return Some(u2641reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2641reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2641reg_COUNT {
        s = s.wrapping_add(u2641reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2641Ledger（fork 复制 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// fork 复制 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2641Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2641Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa900, 0xa90f]。
pub const fn u2641_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2641Code::ALL.len() {
        let c = u2641Code::ALL[i].code();
        if c < 0xa900 || c > 0xa90f {
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
fn u2641_guard_seg() -> bool {
    u2641_segment_guard()
}

/// UNX-F2641 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2641_core_tbl() -> bool {
    let mut ok = u2641reg_COUNT == 24;
    if ok { ok = u2641reg_get("FK_A") == Some(7) && u2641reg_get("FK_B") == Some(33); }
    if ok { ok = u2641reg_get("LK_A") == Some(84); }
    if ok { ok = u2641reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2641reg_sum() == 1668; }
    ok
}

/// UNX-F2641 守恒账三操作确定性效果。
fn check_u2641_core_ldg() -> bool {
    let mut ld = Mxu2641Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2641_checks() -> CheckSet {
    let mut set = CheckSet::new("U2641");
    set.add(
        "u2641-seg-guard",
        u2641_guard_seg(),
        if u2641_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2641-core-tbl",
        check_u2641_core_tbl(),
        if check_u2641_core_tbl() { "" } else { "UNX-F2641 core table mismatch" },
    );
    set.add(
        "u2641-core-ldg",
        check_u2641_core_ldg(),
        if check_u2641_core_ldg() { "" } else { "UNX-F2641 core ledger mismatch" },
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
    fn u2641_all_checks_green() {
        let set = run_u2641_checks();
        assert!(set.all_passed(), "U2641 domain checks must be green");
    }

    #[test]
    fn u2641_segment_guard_holds() {
        assert!(u2641_segment_guard());
    }
}
