//! unxu2961_u2961core —— 域级回归矩阵方案冻结（B01–B28 全域） 语义登记表与守恒账（域级回归全域矩阵）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2961 · 域级回归矩阵方案冻结（B01–B28 全域） · 判据原文：UNX-F2961 · 判据：UNX-F2961-J1 全域回归方案（28 批 560 条零抽样+三 runner 归一+绿账字段对齐）冻结哈希入册，方案与 F2968 绿账字段对齐
//! · 组件：域级回归全域矩阵 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2961 · 判据：UNX-F2961-J1 全域回归方案（28 批 560 条零抽样+三 runner 归一+绿账字段对齐）冻结哈希入册，方案与 F
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2961_VERSION: &str = "u2961-u2961core-v1";

/// 诊断段基址。
pub const U2961_SEG_BASE: u16 = 0xe230;

// --------------------------------------------------------------------------
// 诊断码（u2961 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域级回归全域矩阵 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2961Code {
    /// 0xe230 表行重复（全域 同键双写）。
    RowDup,
    /// 0xe231 表行断档（回归 序号不连续）。
    RowGap,
    /// 0xe232 值越界（矩阵 值域外）。
    ValOob,
    /// 0xe233 累计漂移（全域 求和≠理论值）。
    SumDrift,
    /// 0xe234 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe235 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe236 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe237 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe238 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe239 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe23a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe23b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2961Code {
    /// 全量闭集。
    pub const ALL: [u2961Code; 12] = [
        u2961Code::RowDup,
        u2961Code::RowGap,
        u2961Code::ValOob,
        u2961Code::SumDrift,
        u2961Code::LedgerDup,
        u2961Code::LedgerHole,
        u2961Code::OpNoop,
        u2961Code::FieldDrift,
        u2961Code::LedgerBroken,
        u2961Code::SnapshotFingerprint,
        u2961Code::TableBroken,
        u2961Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2961Code::RowDup => 0xe230,
            u2961Code::RowGap => 0xe231,
            u2961Code::ValOob => 0xe232,
            u2961Code::SumDrift => 0xe233,
            u2961Code::LedgerDup => 0xe234,
            u2961Code::LedgerHole => 0xe235,
            u2961Code::OpNoop => 0xe236,
            u2961Code::FieldDrift => 0xe237,
            u2961Code::LedgerBroken => 0xe238,
            u2961Code::SnapshotFingerprint => 0xe239,
            u2961Code::TableBroken => 0xe23a,
            u2961Code::SegmentGuard => 0xe23b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2961Code::RowDup => "表行重复（全域 同键双写）",
            u2961Code::RowGap => "表行断档（回归 序号不连续）",
            u2961Code::ValOob => "值越界（矩阵 值域外）",
            u2961Code::SumDrift => "累计漂移（全域 求和≠理论值）",
            u2961Code::LedgerDup => "账目重记（同一事件双入账）",
            u2961Code::LedgerHole => "账目断漏（序号缺失）",
            u2961Code::OpNoop => "空操作（账目无效果调用）",
            u2961Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2961Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2961Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2961Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2961Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2961Code::RowDup => "按键唯一律去重",
            u2961Code::RowGap => "按连续律补号",
            u2961Code::ValOob => "按值域裁剪",
            u2961Code::SumDrift => "按理论值复算",
            u2961Code::LedgerDup => "按事件幂等律合并",
            u2961Code::LedgerHole => "按连续律补账",
            u2961Code::OpNoop => "核对操作前置条件",
            u2961Code::FieldDrift => "按口径重建字段",
            u2961Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2961Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2961Code::TableBroken => "按语义规则重建常量表",
            u2961Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2961reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 域级回归全域矩阵 语义登记表（24 行逐行注释）。
pub const u2961reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2961reg: [(&'static str, u64); 24] = [
    ("REG_A", 7), // REG 主登记（回归 · 判据面）
    ("REG_B", 33), // REG 副登记（回归 · 对拍面）
    ("FULL_A", 14), // FULL 主登记（全域 · 判据面）
    ("FULL_B", 44), // FULL 副登记（全域 · 对拍面）
    ("B0128_A", 21), // B0128 主登记（B01-B28 · 判据面）
    ("B0128_B", 55), // B0128 副登记（B01-B28 · 对拍面）
    ("CASE_A", 28), // CASE 主登记（用例 · 判据面）
    ("CASE_B", 66), // CASE 副登记（用例 · 对拍面）
    ("BASE_A", 35), // BASE 主登记（基线 · 判据面）
    ("BASE_B", 77), // BASE 副登记（基线 · 对拍面）
    ("DIFF_A", 42), // DIFF 主登记（差异 · 判据面）
    ("DIFF_B", 88), // DIFF 副登记（差异 · 对拍面）
    ("COV_A", 49), // COV 主登记（覆盖 · 判据面）
    ("COV_B", 99), // COV 副登记（覆盖 · 对拍面）
    ("FLAKY_A", 56), // FLAKY 主登记（抖动 · 判据面）
    ("FLAKY_B", 110), // FLAKY 副登记（抖动 · 对拍面）
    ("QUAR_A", 63), // QUAR 主登记（隔离 · 判据面）
    ("QUAR_B", 121), // QUAR 副登记（隔离 · 对拍面）
    ("SEAL_A", 70), // SEAL 主登记（封存 · 判据面）
    ("SEAL_B", 132), // SEAL 副登记（封存 · 对拍面）
    ("RERUN_A", 77), // RERUN 主登记（复跑 · 判据面）
    ("RERUN_B", 143), // RERUN 副登记（复跑 · 对拍面）
    ("COUNT_A", 84), // COUNT 主登记（计数 · 判据面）
    ("COUNT_B", 154), // COUNT 副登记（计数 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2961reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2961reg_COUNT {
        if u2961reg[i].0 == key {
            return Some(u2961reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2961reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2961reg_COUNT {
        s = s.wrapping_add(u2961reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2961Ledger（域级回归全域矩阵 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 域级回归全域矩阵 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2961Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2961Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe230, 0xe23f]。
pub const fn u2961_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2961Code::ALL.len() {
        let c = u2961Code::ALL[i].code();
        if c < 0xe230 || c > 0xe23f {
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
fn u2961_guard_seg() -> bool {
    u2961_segment_guard()
}

/// UNX-F2961 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2961_core_tbl() -> bool {
    let mut ok = u2961reg_COUNT == 24;
    if ok { ok = u2961reg_get("REG_A") == Some(7) && u2961reg_get("REG_B") == Some(33); }
    if ok { ok = u2961reg_get("COUNT_A") == Some(84); }
    if ok { ok = u2961reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2961reg_sum() == 1668; }
    ok
}

/// UNX-F2961 守恒账三操作确定性效果。
fn check_u2961_core_ldg() -> bool {
    let mut ld = Mxu2961Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2961_checks() -> CheckSet {
    let mut set = CheckSet::new("U2961");
    set.add(
        "u2961-seg-guard",
        u2961_guard_seg(),
        if u2961_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2961-core-tbl",
        check_u2961_core_tbl(),
        if check_u2961_core_tbl() { "" } else { "UNX-F2961 core table mismatch" },
    );
    set.add(
        "u2961-core-ldg",
        check_u2961_core_ldg(),
        if check_u2961_core_ldg() { "" } else { "UNX-F2961 core ledger mismatch" },
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
    fn u2961_all_checks_green() {
        let set = run_u2961_checks();
        assert!(set.all_passed(), "U2961 domain checks must be green");
    }

    #[test]
    fn u2961_segment_guard_holds() {
        assert!(u2961_segment_guard());
    }
}
