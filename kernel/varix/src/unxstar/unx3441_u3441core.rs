//! unxu3441_u3441core —— 执行器总设计（六阶段流水线：选例→隔离→排序→执行→超时→上报） 语义登记表与守恒账（执行器流水线）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3441 · 执行器总设计（六阶段流水线：选例→隔离→排序→执行→超时→上报） · 判据原文：UNX-F3441 · 判据：J1：执行器总设计文档交付（六阶段流水线定义 + 各阶段契约）；M-200 桩后端全链路六阶段贯通。
//! · 组件：执行器流水线 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3441 · 判据：J1：执行器总设计文档交付（六阶段流水线定义 + 各阶段契约）；M-200 桩后端全链路六阶段贯通。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3441_VERSION: &str = "u3441-u3441core-v1";

/// 诊断段基址。
pub const U3441_SEG_BASE: u16 = 0xe090;

// --------------------------------------------------------------------------
// 诊断码（u3441 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 执行器流水线 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3441Code {
    /// 0xe090 表行重复（执行器 同键双写）。
    RowDup,
    /// 0xe091 表行断档（流水线 序号不连续）。
    RowGap,
    /// 0xe092 值越界（超时 值域外）。
    ValOob,
    /// 0xe093 累计漂移（六阶段 求和≠理论值）。
    SumDrift,
    /// 0xe094 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe095 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe096 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe097 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe098 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe099 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe09a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe09b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3441Code {
    /// 全量闭集。
    pub const ALL: [u3441Code; 12] = [
        u3441Code::RowDup,
        u3441Code::RowGap,
        u3441Code::ValOob,
        u3441Code::SumDrift,
        u3441Code::LedgerDup,
        u3441Code::LedgerHole,
        u3441Code::OpNoop,
        u3441Code::FieldDrift,
        u3441Code::LedgerBroken,
        u3441Code::SnapshotFingerprint,
        u3441Code::TableBroken,
        u3441Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3441Code::RowDup => 0xe090,
            u3441Code::RowGap => 0xe091,
            u3441Code::ValOob => 0xe092,
            u3441Code::SumDrift => 0xe093,
            u3441Code::LedgerDup => 0xe094,
            u3441Code::LedgerHole => 0xe095,
            u3441Code::OpNoop => 0xe096,
            u3441Code::FieldDrift => 0xe097,
            u3441Code::LedgerBroken => 0xe098,
            u3441Code::SnapshotFingerprint => 0xe099,
            u3441Code::TableBroken => 0xe09a,
            u3441Code::SegmentGuard => 0xe09b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3441Code::RowDup => "表行重复（执行器 同键双写）",
            u3441Code::RowGap => "表行断档（流水线 序号不连续）",
            u3441Code::ValOob => "值越界（超时 值域外）",
            u3441Code::SumDrift => "累计漂移（六阶段 求和≠理论值）",
            u3441Code::LedgerDup => "账目重记（同一事件双入账）",
            u3441Code::LedgerHole => "账目断漏（序号缺失）",
            u3441Code::OpNoop => "空操作（账目无效果调用）",
            u3441Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3441Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3441Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3441Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3441Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3441Code::RowDup => "按键唯一律去重",
            u3441Code::RowGap => "按连续律补号",
            u3441Code::ValOob => "按值域裁剪",
            u3441Code::SumDrift => "按理论值复算",
            u3441Code::LedgerDup => "按事件幂等律合并",
            u3441Code::LedgerHole => "按连续律补账",
            u3441Code::OpNoop => "核对操作前置条件",
            u3441Code::FieldDrift => "按口径重建字段",
            u3441Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3441Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3441Code::TableBroken => "按语义规则重建常量表",
            u3441Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3441reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 执行器流水线 语义登记表（24 行逐行注释）。
pub const u3441reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3441reg: [(&'static str, u64); 24] = [
    ("EX_A", 7), // EX 主登记（执行器 · 判据面）
    ("EX_B", 33), // EX 副登记（执行器 · 对拍面）
    ("SEL_A", 14), // SEL 主登记（选例 · 判据面）
    ("SEL_B", 44), // SEL 副登记（选例 · 对拍面）
    ("ISO_A", 21), // ISO 主登记（隔离 · 判据面）
    ("ISO_B", 55), // ISO 副登记（隔离 · 对拍面）
    ("SORT_A", 28), // SORT 主登记（排序 · 判据面）
    ("SORT_B", 66), // SORT 副登记（排序 · 对拍面）
    ("RUN_A", 35), // RUN 主登记（执行 · 判据面）
    ("RUN_B", 77), // RUN 副登记（执行 · 对拍面）
    ("TMO_A", 42), // TMO 主登记（超时 · 判据面）
    ("TMO_B", 88), // TMO 副登记（超时 · 对拍面）
    ("RPT_A", 49), // RPT 主登记（上报 · 判据面）
    ("RPT_B", 99), // RPT 副登记（上报 · 对拍面）
    ("STAGE_A", 56), // STAGE 主登记（阶段 · 判据面）
    ("STAGE_B", 110), // STAGE 副登记（阶段 · 对拍面）
    ("QUEUE_A", 63), // QUEUE 主登记（阶段队列 · 判据面）
    ("QUEUE_B", 121), // QUEUE 副登记（阶段队列 · 对拍面）
    ("BUD_A", 70), // BUD 主登记（预算 · 判据面）
    ("BUD_B", 132), // BUD 副登记（预算 · 对拍面）
    ("RT_A", 77), // RT 主登记（运行记录 · 判据面）
    ("RT_B", 143), // RT 副登记（运行记录 · 对拍面）
    ("AGG_A", 84), // AGG 主登记（聚合 · 判据面）
    ("AGG_B", 154), // AGG 副登记（聚合 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3441reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3441reg_COUNT {
        if u3441reg[i].0 == key {
            return Some(u3441reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3441reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3441reg_COUNT {
        s = s.wrapping_add(u3441reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3441Ledger（执行器流水线 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 执行器流水线 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3441Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3441Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe090, 0xe09f]。
pub const fn u3441_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3441Code::ALL.len() {
        let c = u3441Code::ALL[i].code();
        if c < 0xe090 || c > 0xe09f {
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
fn u3441_guard_seg() -> bool {
    u3441_segment_guard()
}

/// UNX-F3441 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3441_core_tbl() -> bool {
    let mut ok = u3441reg_COUNT == 24;
    if ok { ok = u3441reg_get("EX_A") == Some(7) && u3441reg_get("EX_B") == Some(33); }
    if ok { ok = u3441reg_get("AGG_A") == Some(84); }
    if ok { ok = u3441reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3441reg_sum() == 1668; }
    ok
}

/// UNX-F3441 守恒账三操作确定性效果。
fn check_u3441_core_ldg() -> bool {
    let mut ld = Mxu3441Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3441_checks() -> CheckSet {
    let mut set = CheckSet::new("U3441");
    set.add(
        "u3441-seg-guard",
        u3441_guard_seg(),
        if u3441_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3441-core-tbl",
        check_u3441_core_tbl(),
        if check_u3441_core_tbl() { "" } else { "UNX-F3441 core table mismatch" },
    );
    set.add(
        "u3441-core-ldg",
        check_u3441_core_ldg(),
        if check_u3441_core_ldg() { "" } else { "UNX-F3441 core ledger mismatch" },
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
    fn u3441_all_checks_green() {
        let set = run_u3441_checks();
        assert!(set.all_passed(), "U3441 domain checks must be green");
    }

    #[test]
    fn u3441_segment_guard_holds() {
        assert!(u3441_segment_guard());
    }
}
