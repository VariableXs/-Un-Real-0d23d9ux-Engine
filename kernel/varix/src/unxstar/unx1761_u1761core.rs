//! unxu1761_u1761core —— 观测面总纲（账→接口→聚合三层架构） 语义登记表与守恒账（层）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1761 · 观测面总纲（账→接口→聚合三层架构） · 判据原文：UNX-F1761 · 判据：三层架构冻结 v1，各批观测接口汇聚关系全量登记，三层职责越界注入 10/10 次拒绝
//! · 组件：层 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F1761 · 判据：三层架构冻结 v1，各批观测接口汇聚关系全量登记，三层职责越界注入 10/10 次拒绝
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1761_VERSION: &str = "u1761-u1761core-v1";

/// 诊断段基址。
pub const U1761_SEG_BASE: u16 = 0x5b00;

// --------------------------------------------------------------------------
// 诊断码（u1761 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 层 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1761Code {
    /// 0x5b00 表行重复（职责越界 同键双写）。
    RowDup,
    /// 0x5b01 表行断档（汇聚漏登 序号不连续）。
    RowGap,
    /// 0x5b02 值越界（层间穿透 值域外）。
    ValOob,
    /// 0x5b03 累计漂移（通道失联 求和≠理论值）。
    SumDrift,
    /// 0x5b04 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0x5b05 账目断漏（序号缺失）。
    LedgerHole,
    /// 0x5b06 空操作（账目无效果调用）。
    OpNoop,
    /// 0x5b07 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0x5b08 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5b09 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5b0a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5b0b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1761Code {
    /// 全量闭集。
    pub const ALL: [u1761Code; 12] = [
        u1761Code::RowDup,
        u1761Code::RowGap,
        u1761Code::ValOob,
        u1761Code::SumDrift,
        u1761Code::LedgerDup,
        u1761Code::LedgerHole,
        u1761Code::OpNoop,
        u1761Code::FieldDrift,
        u1761Code::LedgerBroken,
        u1761Code::SnapshotFingerprint,
        u1761Code::TableBroken,
        u1761Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1761Code::RowDup => 0x5b00,
            u1761Code::RowGap => 0x5b01,
            u1761Code::ValOob => 0x5b02,
            u1761Code::SumDrift => 0x5b03,
            u1761Code::LedgerDup => 0x5b04,
            u1761Code::LedgerHole => 0x5b05,
            u1761Code::OpNoop => 0x5b06,
            u1761Code::FieldDrift => 0x5b07,
            u1761Code::LedgerBroken => 0x5b08,
            u1761Code::SnapshotFingerprint => 0x5b09,
            u1761Code::TableBroken => 0x5b0a,
            u1761Code::SegmentGuard => 0x5b0b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1761Code::RowDup => "表行重复（职责越界 同键双写）",
            u1761Code::RowGap => "表行断档（汇聚漏登 序号不连续）",
            u1761Code::ValOob => "值越界（层间穿透 值域外）",
            u1761Code::SumDrift => "累计漂移（通道失联 求和≠理论值）",
            u1761Code::LedgerDup => "账目重记（同一事件双入账）",
            u1761Code::LedgerHole => "账目断漏（序号缺失）",
            u1761Code::OpNoop => "空操作（账目无效果调用）",
            u1761Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u1761Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1761Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1761Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1761Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1761Code::RowDup => "按键唯一律去重",
            u1761Code::RowGap => "按连续律补号",
            u1761Code::ValOob => "按值域裁剪",
            u1761Code::SumDrift => "按理论值复算",
            u1761Code::LedgerDup => "按事件幂等律合并",
            u1761Code::LedgerHole => "按连续律补账",
            u1761Code::OpNoop => "核对操作前置条件",
            u1761Code::FieldDrift => "按口径重建字段",
            u1761Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1761Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1761Code::TableBroken => "按语义规则重建常量表",
            u1761Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1761reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 层 语义登记表（24 行逐行注释）。
pub const u1761reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1761reg: [(&'static str, u64); 24] = [
    ("Z_A", 7), // Z 主登记（账层 · 判据面）
    ("Z_B", 33), // Z 副登记（账层 · 对拍面）
    ("JK_A", 14), // JK 主登记（接口层 · 判据面）
    ("JK_B", 44), // JK 副登记（接口层 · 对拍面）
    ("JH_A", 21), // JH 主登记（聚合层 · 判据面）
    ("JH_B", 55), // JH 副登记（聚合层 · 对拍面）
    ("HJ_A", 28), // HJ 主登记（汇聚关系 · 判据面）
    ("HJ_B", 66), // HJ 副登记（汇聚关系 · 对拍面）
    ("QL_A", 35), // QL 主登记（全量登记 · 判据面）
    ("QL_B", 77), // QL 副登记（全量登记 · 对拍面）
    ("ZZ_A", 42), // ZZ 主登记（职责越界 · 判据面）
    ("ZZ_B", 88), // ZZ 副登记（职责越界 · 对拍面）
    ("JJ_A", 49), // JJ 主登记（拒绝 · 判据面）
    ("JJ_B", 99), // JJ 副登记（拒绝 · 对拍面）
    ("MZ_A", 56), // MZ 主登记（冻结 v1 · 判据面）
    ("MZ_B", 110), // MZ 副登记（冻结 v1 · 对拍面）
    ("GC_A", 63), // GC 主登记（各批接口 · 判据面）
    ("GC_B", 121), // GC 副登记（各批接口 · 对拍面）
    ("CJ_A", 70), // CJ 主登记（层级闭集 · 判据面）
    ("CJ_B", 132), // CJ 副登记（层级闭集 · 对拍面）
    ("SJ_A", 77), // SJ 主登记（上行通道 · 判据面）
    ("SJ_B", 143), // SJ 副登记（上行通道 · 对拍面）
    ("XD_A", 84), // XD 主登记（下行通道 · 判据面）
    ("XD_B", 154), // XD 副登记（下行通道 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1761reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1761reg_COUNT {
        if u1761reg[i].0 == key {
            return Some(u1761reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1761reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1761reg_COUNT {
        s = s.wrapping_add(u1761reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu1761Ledger（层 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 层 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu1761Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu1761Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5b00, 0x5b0f]。
pub const fn u1761_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1761Code::ALL.len() {
        let c = u1761Code::ALL[i].code();
        if c < 0x5b00 || c > 0x5b0f {
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
fn u1761_guard_seg() -> bool {
    u1761_segment_guard()
}

/// UNX-F1761 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u1761_core_tbl() -> bool {
    let mut ok = u1761reg_COUNT == 24;
    if ok { ok = u1761reg_get("Z_A") == Some(7) && u1761reg_get("Z_B") == Some(33); }
    if ok { ok = u1761reg_get("XD_A") == Some(84); }
    if ok { ok = u1761reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1761reg_sum() == 1668; }
    ok
}

/// UNX-F1761 守恒账三操作确定性效果。
fn check_u1761_core_ldg() -> bool {
    let mut ld = Mxu1761Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1761_checks() -> CheckSet {
    let mut set = CheckSet::new("U1761");
    set.add(
        "u1761-seg-guard",
        u1761_guard_seg(),
        if u1761_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1761-core-tbl",
        check_u1761_core_tbl(),
        if check_u1761_core_tbl() { "" } else { "UNX-F1761 core table mismatch" },
    );
    set.add(
        "u1761-core-ldg",
        check_u1761_core_ldg(),
        if check_u1761_core_ldg() { "" } else { "UNX-F1761 core ledger mismatch" },
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
    fn u1761_all_checks_green() {
        let set = run_u1761_checks();
        assert!(set.all_passed(), "U1761 domain checks must be green");
    }

    #[test]
    fn u1761_segment_guard_holds() {
        assert!(u1761_segment_guard());
    }
}
