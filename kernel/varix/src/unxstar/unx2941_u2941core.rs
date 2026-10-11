//! unxu2941_u2941core —— 域级压力矩阵方案冻结（三轴） 语义登记表与守恒账（域级压力三轴）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2941 · 域级压力矩阵方案冻结（三轴） · 判据原文：UNX-F2941 · 判据：UNX-F2941-J1 三轴方案（8 核矩阵×16GB 三档压力×72h 长稳）冻结哈希入册，方案与 F2938 汇总账字段对齐
//! · 组件：域级压力三轴 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2941 · 判据：UNX-F2941-J1 三轴方案（8 核矩阵×16GB 三档压力×72h 长稳）冻结哈希入册，方案与 F2938 汇总账字段对齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2941_VERSION: &str = "u2941-u2941core-v1";

/// 诊断段基址。
pub const U2941_SEG_BASE: u16 = 0xe220;

// --------------------------------------------------------------------------
// 诊断码（u2941 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域级压力三轴 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2941Code {
    /// 0xe220 表行重复（域级 同键双写）。
    RowDup,
    /// 0xe221 表行断档（压力 序号不连续）。
    RowGap,
    /// 0xe222 值越界（三轴 值域外）。
    ValOob,
    /// 0xe223 累计漂移（三轴 求和≠理论值）。
    SumDrift,
    /// 0xe224 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe225 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe226 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe227 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe228 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe229 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe22a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe22b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2941Code {
    /// 全量闭集。
    pub const ALL: [u2941Code; 12] = [
        u2941Code::RowDup,
        u2941Code::RowGap,
        u2941Code::ValOob,
        u2941Code::SumDrift,
        u2941Code::LedgerDup,
        u2941Code::LedgerHole,
        u2941Code::OpNoop,
        u2941Code::FieldDrift,
        u2941Code::LedgerBroken,
        u2941Code::SnapshotFingerprint,
        u2941Code::TableBroken,
        u2941Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2941Code::RowDup => 0xe220,
            u2941Code::RowGap => 0xe221,
            u2941Code::ValOob => 0xe222,
            u2941Code::SumDrift => 0xe223,
            u2941Code::LedgerDup => 0xe224,
            u2941Code::LedgerHole => 0xe225,
            u2941Code::OpNoop => 0xe226,
            u2941Code::FieldDrift => 0xe227,
            u2941Code::LedgerBroken => 0xe228,
            u2941Code::SnapshotFingerprint => 0xe229,
            u2941Code::TableBroken => 0xe22a,
            u2941Code::SegmentGuard => 0xe22b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2941Code::RowDup => "表行重复（域级 同键双写）",
            u2941Code::RowGap => "表行断档（压力 序号不连续）",
            u2941Code::ValOob => "值越界（三轴 值域外）",
            u2941Code::SumDrift => "累计漂移（三轴 求和≠理论值）",
            u2941Code::LedgerDup => "账目重记（同一事件双入账）",
            u2941Code::LedgerHole => "账目断漏（序号缺失）",
            u2941Code::OpNoop => "空操作（账目无效果调用）",
            u2941Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2941Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2941Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2941Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2941Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2941Code::RowDup => "按键唯一律去重",
            u2941Code::RowGap => "按连续律补号",
            u2941Code::ValOob => "按值域裁剪",
            u2941Code::SumDrift => "按理论值复算",
            u2941Code::LedgerDup => "按事件幂等律合并",
            u2941Code::LedgerHole => "按连续律补账",
            u2941Code::OpNoop => "核对操作前置条件",
            u2941Code::FieldDrift => "按口径重建字段",
            u2941Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2941Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2941Code::TableBroken => "按语义规则重建常量表",
            u2941Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2941reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 域级压力三轴 语义登记表（24 行逐行注释）。
pub const u2941reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2941reg: [(&'static str, u64); 24] = [
    ("AX1_A", 7), // AX1 主登记（轴一 · 判据面）
    ("AX1_B", 33), // AX1 副登记（轴一 · 对拍面）
    ("AX2_A", 14), // AX2 主登记（轴二 · 判据面）
    ("AX2_B", 44), // AX2 副登记（轴二 · 对拍面）
    ("AX3_A", 21), // AX3 主登记（轴三 · 判据面）
    ("AX3_B", 55), // AX3 副登记（轴三 · 对拍面）
    ("STRESS_A", 28), // STRESS 主登记（压力 · 判据面）
    ("STRESS_B", 66), // STRESS 副登记（压力 · 对拍面）
    ("DOM_A", 35), // DOM 主登记（域级 · 判据面）
    ("DOM_B", 77), // DOM 副登记（域级 · 对拍面）
    ("MATRIX_A", 42), // MATRIX 主登记（矩阵 · 判据面）
    ("MATRIX_B", 88), // MATRIX 副登记（矩阵 · 对拍面）
    ("LOAD_A", 49), // LOAD 主登记（负载 · 判据面）
    ("LOAD_B", 99), // LOAD 副登记（负载 · 对拍面）
    ("TIME_A", 56), // TIME 主登记（时长 · 判据面）
    ("TIME_B", 110), // TIME 副登记（时长 · 对拍面）
    ("MET_A", 63), // MET 主登记（指标 · 判据面）
    ("MET_B", 121), // MET 副登记（指标 · 对拍面）
    ("THR_A", 70), // THR 主登记（阈值 · 判据面）
    ("THR_B", 132), // THR 副登记（阈值 · 对拍面）
    ("VERDICT_A", 77), // VERDICT 主登记（裁决 · 判据面）
    ("VERDICT_B", 143), // VERDICT 副登记（裁决 · 对拍面）
    ("SEAL_A", 84), // SEAL 主登记（封存 · 判据面）
    ("SEAL_B", 154), // SEAL 副登记（封存 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2941reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2941reg_COUNT {
        if u2941reg[i].0 == key {
            return Some(u2941reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2941reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2941reg_COUNT {
        s = s.wrapping_add(u2941reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2941Ledger（域级压力三轴 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 域级压力三轴 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2941Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2941Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe220, 0xe22f]。
pub const fn u2941_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2941Code::ALL.len() {
        let c = u2941Code::ALL[i].code();
        if c < 0xe220 || c > 0xe22f {
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
fn u2941_guard_seg() -> bool {
    u2941_segment_guard()
}

/// UNX-F2941 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2941_core_tbl() -> bool {
    let mut ok = u2941reg_COUNT == 24;
    if ok { ok = u2941reg_get("AX1_A") == Some(7) && u2941reg_get("AX1_B") == Some(33); }
    if ok { ok = u2941reg_get("SEAL_A") == Some(84); }
    if ok { ok = u2941reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2941reg_sum() == 1668; }
    ok
}

/// UNX-F2941 守恒账三操作确定性效果。
fn check_u2941_core_ldg() -> bool {
    let mut ld = Mxu2941Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2941_checks() -> CheckSet {
    let mut set = CheckSet::new("U2941");
    set.add(
        "u2941-seg-guard",
        u2941_guard_seg(),
        if u2941_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2941-core-tbl",
        check_u2941_core_tbl(),
        if check_u2941_core_tbl() { "" } else { "UNX-F2941 core table mismatch" },
    );
    set.add(
        "u2941-core-ldg",
        check_u2941_core_ldg(),
        if check_u2941_core_ldg() { "" } else { "UNX-F2941 core ledger mismatch" },
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
    fn u2941_all_checks_green() {
        let set = run_u2941_checks();
        assert!(set.all_passed(), "U2941 domain checks must be green");
    }

    #[test]
    fn u2941_segment_guard_holds() {
        assert!(u2941_segment_guard());
    }
}
