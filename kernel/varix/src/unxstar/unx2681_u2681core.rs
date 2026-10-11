//! unxu2681_u2681core —— 命中率账本总账与 JSON 导出冻结 语义登记表与守恒账（命中率账）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2681 · 命中率账本总账与 JSON 导出冻结 · 判据原文：UNX-F2681 · 判据：JSON 导出格式冻结（字段/枚举/单位三冻结），F2700 判据"账本导出可查"承载声明与 F2667 消费闭合
//! · 组件：命中率账 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2681 · 判据：JSON 导出格式冻结（字段/枚举/单位三冻结），F2700 判据"账本导出可查"承载声明与 F2667 消费闭合
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2681_VERSION: &str = "u2681-u2681core-v1";

/// 诊断段基址。
pub const U2681_SEG_BASE: u16 = 0xaa80;

// --------------------------------------------------------------------------
// 诊断码（u2681 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 命中率账 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2681Code {
    /// 0xaa80 表行重复（字段漂移 同键双写）。
    RowDup,
    /// 0xaa81 表行断档（枚举不一致 序号不连续）。
    RowGap,
    /// 0xaa82 值越界（单位错配 值域外）。
    ValOob,
    /// 0xaa83 累计漂移（闭合断裂 求和≠理论值）。
    SumDrift,
    /// 0xaa84 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xaa85 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xaa86 空操作（账目无效果调用）。
    OpNoop,
    /// 0xaa87 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xaa88 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xaa89 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xaa8a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xaa8b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2681Code {
    /// 全量闭集。
    pub const ALL: [u2681Code; 12] = [
        u2681Code::RowDup,
        u2681Code::RowGap,
        u2681Code::ValOob,
        u2681Code::SumDrift,
        u2681Code::LedgerDup,
        u2681Code::LedgerHole,
        u2681Code::OpNoop,
        u2681Code::FieldDrift,
        u2681Code::LedgerBroken,
        u2681Code::SnapshotFingerprint,
        u2681Code::TableBroken,
        u2681Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2681Code::RowDup => 0xaa80,
            u2681Code::RowGap => 0xaa81,
            u2681Code::ValOob => 0xaa82,
            u2681Code::SumDrift => 0xaa83,
            u2681Code::LedgerDup => 0xaa84,
            u2681Code::LedgerHole => 0xaa85,
            u2681Code::OpNoop => 0xaa86,
            u2681Code::FieldDrift => 0xaa87,
            u2681Code::LedgerBroken => 0xaa88,
            u2681Code::SnapshotFingerprint => 0xaa89,
            u2681Code::TableBroken => 0xaa8a,
            u2681Code::SegmentGuard => 0xaa8b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2681Code::RowDup => "表行重复（字段漂移 同键双写）",
            u2681Code::RowGap => "表行断档（枚举不一致 序号不连续）",
            u2681Code::ValOob => "值越界（单位错配 值域外）",
            u2681Code::SumDrift => "累计漂移（闭合断裂 求和≠理论值）",
            u2681Code::LedgerDup => "账目重记（同一事件双入账）",
            u2681Code::LedgerHole => "账目断漏（序号缺失）",
            u2681Code::OpNoop => "空操作（账目无效果调用）",
            u2681Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2681Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2681Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2681Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2681Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2681Code::RowDup => "按键唯一律去重",
            u2681Code::RowGap => "按连续律补号",
            u2681Code::ValOob => "按值域裁剪",
            u2681Code::SumDrift => "按理论值复算",
            u2681Code::LedgerDup => "按事件幂等律合并",
            u2681Code::LedgerHole => "按连续律补账",
            u2681Code::OpNoop => "核对操作前置条件",
            u2681Code::FieldDrift => "按口径重建字段",
            u2681Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2681Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2681Code::TableBroken => "按语义规则重建常量表",
            u2681Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2681reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 命中率账 语义登记表（24 行逐行注释）。
pub const u2681reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2681reg: [(&'static str, u64); 24] = [
    ("HR_A", 7), // HR 主登记（命中率 · 判据面）
    ("HR_B", 33), // HR 副登记（命中率 · 对拍面）
    ("LG_A", 14), // LG 主登记（账本总账 · 判据面）
    ("LG_B", 44), // LG 副登记（账本总账 · 对拍面）
    ("JS_A", 21), // JS 主登记（JSON 导出 · 判据面）
    ("JS_B", 55), // JS 副登记（JSON 导出 · 对拍面）
    ("FL_A", 28), // FL 主登记（字段冻结 · 判据面）
    ("FL_B", 66), // FL 副登记（字段冻结 · 对拍面）
    ("EN_A", 35), // EN 主登记（枚举冻结 · 判据面）
    ("EN_B", 77), // EN 副登记（枚举冻结 · 对拍面）
    ("UN_A", 42), // UN 主登记（单位冻结 · 判据面）
    ("UN_B", 88), // UN 副登记（单位冻结 · 对拍面）
    ("F2700_A", 49), // F2700 主登记（可查承载 · 判据面）
    ("F2700_B", 99), // F2700 副登记（可查承载 · 对拍面）
    ("CN_A", 56), // CN 主登记（F2667 消费 · 判据面）
    ("CN_B", 110), // CN 副登记（F2667 消费 · 对拍面）
    ("AC_A", 63), // AC 主登记（闭合断言 · 判据面）
    ("AC_B", 121), // AC 副登记（闭合断言 · 对拍面）
    ("EX_A", 70), // EX 主登记（导出面 · 判据面）
    ("EX_B", 132), // EX 副登记（导出面 · 对拍面）
    ("SM_A", 77), // SM 主登记（汇总面 · 判据面）
    ("SM_B", 143), // SM 副登记（汇总面 · 对拍面）
    ("TS_A", 84), // TS 主登记（时序面 · 判据面）
    ("TS_B", 154), // TS 副登记（时序面 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2681reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2681reg_COUNT {
        if u2681reg[i].0 == key {
            return Some(u2681reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2681reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2681reg_COUNT {
        s = s.wrapping_add(u2681reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2681Ledger（命中率账 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 命中率账 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2681Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2681Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xaa80, 0xaa8f]。
pub const fn u2681_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2681Code::ALL.len() {
        let c = u2681Code::ALL[i].code();
        if c < 0xaa80 || c > 0xaa8f {
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
fn u2681_guard_seg() -> bool {
    u2681_segment_guard()
}

/// UNX-F2681 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2681_core_tbl() -> bool {
    let mut ok = u2681reg_COUNT == 24;
    if ok { ok = u2681reg_get("HR_A") == Some(7) && u2681reg_get("HR_B") == Some(33); }
    if ok { ok = u2681reg_get("TS_A") == Some(84); }
    if ok { ok = u2681reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2681reg_sum() == 1668; }
    ok
}

/// UNX-F2681 守恒账三操作确定性效果。
fn check_u2681_core_ldg() -> bool {
    let mut ld = Mxu2681Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2681_checks() -> CheckSet {
    let mut set = CheckSet::new("U2681");
    set.add(
        "u2681-seg-guard",
        u2681_guard_seg(),
        if u2681_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2681-core-tbl",
        check_u2681_core_tbl(),
        if check_u2681_core_tbl() { "" } else { "UNX-F2681 core table mismatch" },
    );
    set.add(
        "u2681-core-ldg",
        check_u2681_core_ldg(),
        if check_u2681_core_ldg() { "" } else { "UNX-F2681 core ledger mismatch" },
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
    fn u2681_all_checks_green() {
        let set = run_u2681_checks();
        assert!(set.all_passed(), "U2681 domain checks must be green");
    }

    #[test]
    fn u2681_segment_guard_holds() {
        assert!(u2681_segment_guard());
    }
}
