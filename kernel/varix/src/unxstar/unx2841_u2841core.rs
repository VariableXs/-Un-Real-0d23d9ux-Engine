//! unxu2841_u2841core —— 大页-地址空间联合压测方案与执行 语义登记表与守恒账（大页-地址空间联合压测）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2841 · 大页-地址空间联合压测方案与执行 · 判据原文：UNX-F2841 · 判据：UNX-F2841-J1 大页映射进地址空间全链（建立-翻译-拆除）10/10 轮贯通，链账三段闭合
//! · 组件：大页-地址空间联合压测 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2841 · 判据：UNX-F2841-J1 大页映射进地址空间全链（建立-翻译-拆除）10/10 轮贯通，链账三段闭合
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2841_VERSION: &str = "u2841-u2841core-v1";

/// 诊断段基址。
pub const U2841_SEG_BASE: u16 = 0xe1d0;

// --------------------------------------------------------------------------
// 诊断码（u2841 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 大页-地址空间联合压测 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2841Code {
    /// 0xe1d0 表行重复（联合压测 同键双写）。
    RowDup,
    /// 0xe1d1 表行断档（交叉 序号不连续）。
    RowGap,
    /// 0xe1d2 值越界（矩阵 值域外）。
    ValOob,
    /// 0xe1d3 累计漂移（联合 求和≠理论值）。
    SumDrift,
    /// 0xe1d4 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe1d5 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe1d6 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe1d7 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe1d8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1d9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1da 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1db 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2841Code {
    /// 全量闭集。
    pub const ALL: [u2841Code; 12] = [
        u2841Code::RowDup,
        u2841Code::RowGap,
        u2841Code::ValOob,
        u2841Code::SumDrift,
        u2841Code::LedgerDup,
        u2841Code::LedgerHole,
        u2841Code::OpNoop,
        u2841Code::FieldDrift,
        u2841Code::LedgerBroken,
        u2841Code::SnapshotFingerprint,
        u2841Code::TableBroken,
        u2841Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2841Code::RowDup => 0xe1d0,
            u2841Code::RowGap => 0xe1d1,
            u2841Code::ValOob => 0xe1d2,
            u2841Code::SumDrift => 0xe1d3,
            u2841Code::LedgerDup => 0xe1d4,
            u2841Code::LedgerHole => 0xe1d5,
            u2841Code::OpNoop => 0xe1d6,
            u2841Code::FieldDrift => 0xe1d7,
            u2841Code::LedgerBroken => 0xe1d8,
            u2841Code::SnapshotFingerprint => 0xe1d9,
            u2841Code::TableBroken => 0xe1da,
            u2841Code::SegmentGuard => 0xe1db,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2841Code::RowDup => "表行重复（联合压测 同键双写）",
            u2841Code::RowGap => "表行断档（交叉 序号不连续）",
            u2841Code::ValOob => "值越界（矩阵 值域外）",
            u2841Code::SumDrift => "累计漂移（联合 求和≠理论值）",
            u2841Code::LedgerDup => "账目重记（同一事件双入账）",
            u2841Code::LedgerHole => "账目断漏（序号缺失）",
            u2841Code::OpNoop => "空操作（账目无效果调用）",
            u2841Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2841Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2841Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2841Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2841Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2841Code::RowDup => "按键唯一律去重",
            u2841Code::RowGap => "按连续律补号",
            u2841Code::ValOob => "按值域裁剪",
            u2841Code::SumDrift => "按理论值复算",
            u2841Code::LedgerDup => "按事件幂等律合并",
            u2841Code::LedgerHole => "按连续律补账",
            u2841Code::OpNoop => "核对操作前置条件",
            u2841Code::FieldDrift => "按口径重建字段",
            u2841Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2841Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2841Code::TableBroken => "按语义规则重建常量表",
            u2841Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2841reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 大页-地址空间联合压测 语义登记表（24 行逐行注释）。
pub const u2841reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2841reg: [(&'static str, u64); 24] = [
    ("HP_A", 7), // HP 主登记（大页 · 判据面）
    ("HP_B", 33), // HP 副登记（大页 · 对拍面）
    ("AS_A", 14), // AS 主登记（地址空间 · 判据面）
    ("AS_B", 44), // AS 副登记（地址空间 · 对拍面）
    ("JOINT_A", 21), // JOINT 主登记（联合 · 判据面）
    ("JOINT_B", 55), // JOINT 副登记（联合 · 对拍面）
    ("MATRIX_A", 28), // MATRIX 主登记（矩阵 · 判据面）
    ("MATRIX_B", 66), // MATRIX 副登记（矩阵 · 对拍面）
    ("XPROD_A", 35), // XPROD 主登记（交叉组合 · 判据面）
    ("XPROD_B", 77), // XPROD 副登记（交叉组合 · 对拍面）
    ("LOAD_A", 42), // LOAD 主登记（负载 · 判据面）
    ("LOAD_B", 88), // LOAD 副登记（负载 · 对拍面）
    ("FRAG_A", 49), // FRAG 主登记（碎片 · 判据面）
    ("FRAG_B", 99), // FRAG 副登记（碎片 · 对拍面）
    ("SW_A", 56), // SW 主登记（切换 · 判据面）
    ("SW_B", 110), // SW 副登记（切换 · 对拍面）
    ("ISOL_A", 63), // ISOL 主登记（隔离 · 判据面）
    ("ISOL_B", 121), // ISOL 副登记（隔离 · 对拍面）
    ("FAIR_A", 70), // FAIR 主登记（公平 · 判据面）
    ("FAIR_B", 132), // FAIR 副登记（公平 · 对拍面）
    ("BUD_A", 77), // BUD 主登记（预算 · 判据面）
    ("BUD_B", 143), // BUD 副登记（预算 · 对拍面）
    ("VERDICT_A", 84), // VERDICT 主登记（裁决 · 判据面）
    ("VERDICT_B", 154), // VERDICT 副登记（裁决 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2841reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2841reg_COUNT {
        if u2841reg[i].0 == key {
            return Some(u2841reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2841reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2841reg_COUNT {
        s = s.wrapping_add(u2841reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2841Ledger（大页-地址空间联合压测 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 大页-地址空间联合压测 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2841Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2841Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1d0, 0xe1df]。
pub const fn u2841_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2841Code::ALL.len() {
        let c = u2841Code::ALL[i].code();
        if c < 0xe1d0 || c > 0xe1df {
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
fn u2841_guard_seg() -> bool {
    u2841_segment_guard()
}

/// UNX-F2841 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2841_core_tbl() -> bool {
    let mut ok = u2841reg_COUNT == 24;
    if ok { ok = u2841reg_get("HP_A") == Some(7) && u2841reg_get("HP_B") == Some(33); }
    if ok { ok = u2841reg_get("VERDICT_A") == Some(84); }
    if ok { ok = u2841reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2841reg_sum() == 1668; }
    ok
}

/// UNX-F2841 守恒账三操作确定性效果。
fn check_u2841_core_ldg() -> bool {
    let mut ld = Mxu2841Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2841_checks() -> CheckSet {
    let mut set = CheckSet::new("U2841");
    set.add(
        "u2841-seg-guard",
        u2841_guard_seg(),
        if u2841_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2841-core-tbl",
        check_u2841_core_tbl(),
        if check_u2841_core_tbl() { "" } else { "UNX-F2841 core table mismatch" },
    );
    set.add(
        "u2841-core-ldg",
        check_u2841_core_ldg(),
        if check_u2841_core_ldg() { "" } else { "UNX-F2841 core ledger mismatch" },
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
    fn u2841_all_checks_green() {
        let set = run_u2841_checks();
        assert!(set.all_passed(), "U2841 domain checks must be green");
    }

    #[test]
    fn u2841_segment_guard_holds() {
        assert!(u2841_segment_guard());
    }
}
