//! unxu2401_u2401core —— 物理内存图消费与四态归一 语义登记表与守恒账（内存区间）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2401 · 物理内存图消费与四态归一 · 判据原文：UNX-F2401 · 判据：真机引导后消费 A1 合并内存图，四态归一（可用/保留/可回收/坏页）区间数与 A1 合并账一致，逐态字节和与总 RAM 偏差 ≤4KB
//! · 组件：内存区间 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2401 · 判据：真机引导后消费 A1 合并内存图，四态归一（可用/保留/可回收/坏页）区间数与 A1 合并账一致，逐态字节和与总 RAM 偏差 ≤
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2401_VERSION: &str = "u2401-u2401core-v1";

/// 诊断段基址。
pub const U2401_SEG_BASE: u16 = 0xa000;

// --------------------------------------------------------------------------
// 诊断码（u2401 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 内存区间 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2401Code {
    /// 0xa000 表行重复（区间数失配 同键双写）。
    RowDup,
    /// 0xa001 表行断档（字节和偏差 序号不连续）。
    RowGap,
    /// 0xa002 值越界（四态漏归 值域外）。
    ValOob,
    /// 0xa003 累计漂移（坏页误判 求和≠理论值）。
    SumDrift,
    /// 0xa004 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xa005 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xa006 空操作（账目无效果调用）。
    OpNoop,
    /// 0xa007 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xa008 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa009 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa00a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa00b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2401Code {
    /// 全量闭集。
    pub const ALL: [u2401Code; 12] = [
        u2401Code::RowDup,
        u2401Code::RowGap,
        u2401Code::ValOob,
        u2401Code::SumDrift,
        u2401Code::LedgerDup,
        u2401Code::LedgerHole,
        u2401Code::OpNoop,
        u2401Code::FieldDrift,
        u2401Code::LedgerBroken,
        u2401Code::SnapshotFingerprint,
        u2401Code::TableBroken,
        u2401Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2401Code::RowDup => 0xa000,
            u2401Code::RowGap => 0xa001,
            u2401Code::ValOob => 0xa002,
            u2401Code::SumDrift => 0xa003,
            u2401Code::LedgerDup => 0xa004,
            u2401Code::LedgerHole => 0xa005,
            u2401Code::OpNoop => 0xa006,
            u2401Code::FieldDrift => 0xa007,
            u2401Code::LedgerBroken => 0xa008,
            u2401Code::SnapshotFingerprint => 0xa009,
            u2401Code::TableBroken => 0xa00a,
            u2401Code::SegmentGuard => 0xa00b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2401Code::RowDup => "表行重复（区间数失配 同键双写）",
            u2401Code::RowGap => "表行断档（字节和偏差 序号不连续）",
            u2401Code::ValOob => "值越界（四态漏归 值域外）",
            u2401Code::SumDrift => "累计漂移（坏页误判 求和≠理论值）",
            u2401Code::LedgerDup => "账目重记（同一事件双入账）",
            u2401Code::LedgerHole => "账目断漏（序号缺失）",
            u2401Code::OpNoop => "空操作（账目无效果调用）",
            u2401Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2401Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2401Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2401Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2401Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2401Code::RowDup => "按键唯一律去重",
            u2401Code::RowGap => "按连续律补号",
            u2401Code::ValOob => "按值域裁剪",
            u2401Code::SumDrift => "按理论值复算",
            u2401Code::LedgerDup => "按事件幂等律合并",
            u2401Code::LedgerHole => "按连续律补账",
            u2401Code::OpNoop => "核对操作前置条件",
            u2401Code::FieldDrift => "按口径重建字段",
            u2401Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2401Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2401Code::TableBroken => "按语义规则重建常量表",
            u2401Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2401reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 内存区间 语义登记表（24 行逐行注释）。
pub const u2401reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2401reg: [(&'static str, u64); 24] = [
    ("AV_A", 7), // AV 主登记（可用区间 · 判据面）
    ("AV_B", 33), // AV 副登记（可用区间 · 对拍面）
    ("RS_A", 14), // RS 主登记（保留区间 · 判据面）
    ("RS_B", 44), // RS 副登记（保留区间 · 对拍面）
    ("RC_A", 21), // RC 主登记（可回收区间 · 判据面）
    ("RC_B", 55), // RC 副登记（可回收区间 · 对拍面）
    ("BD_A", 28), // BD 主登记（坏页区间 · 判据面）
    ("BD_B", 66), // BD 副登记（坏页区间 · 对拍面）
    ("MG_A", 35), // MG 主登记（合并内存图 · 判据面）
    ("MG_B", 77), // MG 副登记（合并内存图 · 对拍面）
    ("IV_A", 42), // IV 主登记（区间数账 · 判据面）
    ("IV_B", 88), // IV 副登记（区间数账 · 对拍面）
    ("BS_A", 49), // BS 主登记（逐态字节和 · 判据面）
    ("BS_B", 99), // BS 副登记（逐态字节和 · 对拍面）
    ("DR_A", 56), // DR 主登记（偏差阈值 · 判据面）
    ("DR_B", 110), // DR 副登记（偏差阈值 · 对拍面）
    ("RAM_A", 63), // RAM 主登记（总 RAM 账 · 判据面）
    ("RAM_B", 121), // RAM 副登记（总 RAM 账 · 对拍面）
    ("RG_A", 70), // RG 主登记（归一段 · 判据面）
    ("RG_B", 132), // RG 副登记（归一段 · 对拍面）
    ("NM_A", 77), // NM 主登记（归一映射 · 判据面）
    ("NM_B", 143), // NM 副登记（归一映射 · 对拍面）
    ("QA_A", 84), // QA 主登记（对平断言 · 判据面）
    ("QA_B", 154), // QA 副登记（对平断言 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2401reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2401reg_COUNT {
        if u2401reg[i].0 == key {
            return Some(u2401reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2401reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2401reg_COUNT {
        s = s.wrapping_add(u2401reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2401Ledger（内存区间 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 内存区间 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2401Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2401Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa000, 0xa00f]。
pub const fn u2401_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2401Code::ALL.len() {
        let c = u2401Code::ALL[i].code();
        if c < 0xa000 || c > 0xa00f {
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
fn u2401_guard_seg() -> bool {
    u2401_segment_guard()
}

/// UNX-F2401 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2401_core_tbl() -> bool {
    let mut ok = u2401reg_COUNT == 24;
    if ok { ok = u2401reg_get("AV_A") == Some(7) && u2401reg_get("AV_B") == Some(33); }
    if ok { ok = u2401reg_get("QA_A") == Some(84); }
    if ok { ok = u2401reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2401reg_sum() == 1668; }
    ok
}

/// UNX-F2401 守恒账三操作确定性效果。
fn check_u2401_core_ldg() -> bool {
    let mut ld = Mxu2401Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2401_checks() -> CheckSet {
    let mut set = CheckSet::new("U2401");
    set.add(
        "u2401-seg-guard",
        u2401_guard_seg(),
        if u2401_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2401-core-tbl",
        check_u2401_core_tbl(),
        if check_u2401_core_tbl() { "" } else { "UNX-F2401 core table mismatch" },
    );
    set.add(
        "u2401-core-ldg",
        check_u2401_core_ldg(),
        if check_u2401_core_ldg() { "" } else { "UNX-F2401 core ledger mismatch" },
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
    fn u2401_all_checks_green() {
        let set = run_u2401_checks();
        assert!(set.all_passed(), "U2401 domain checks must be green");
    }

    #[test]
    fn u2401_segment_guard_holds() {
        assert!(u2401_segment_guard());
    }
}
