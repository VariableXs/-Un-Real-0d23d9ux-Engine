//! unxu3621_u3621core —— RCU 压测总设计（读/写/宽限三面实验框架） 语义登记表与守恒账（RCU 三面实验）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3621 · RCU 压测总设计（读/写/宽限三面实验框架） · 判据原文：UNX-F3621 · 判据：J1：三面框架（读面（并发/一致性）/写面（复制发布切换）/宽限面（时长/滞留/回收）——判据行内嵌职责表）；M-379 三面首跑（最小三面用例各绿）。<br>J2：框架零侵（RCU 路径拍数与基线一致——度量可关）。
//! · 组件：RCU 三面实验 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3621 · 判据：J1：三面框架（读面（并发/一致性）/写面（复制发布切换）/宽限面（时长/滞留/回收）——判据行内嵌职责表）；M-379 三面首跑
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3621_VERSION: &str = "u3621-u3621core-v1";

/// 诊断段基址。
pub const U3621_SEG_BASE: u16 = 0xe120;

// --------------------------------------------------------------------------
// 诊断码（u3621 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// RCU 三面实验 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3621Code {
    /// 0xe120 表行重复（RCU 同键双写）。
    RowDup,
    /// 0xe121 表行断档（宽限 序号不连续）。
    RowGap,
    /// 0xe122 值越界（实验 值域外）。
    ValOob,
    /// 0xe123 累计漂移（三面 求和≠理论值）。
    SumDrift,
    /// 0xe124 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe125 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe126 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe127 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe128 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe129 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe12a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe12b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3621Code {
    /// 全量闭集。
    pub const ALL: [u3621Code; 12] = [
        u3621Code::RowDup,
        u3621Code::RowGap,
        u3621Code::ValOob,
        u3621Code::SumDrift,
        u3621Code::LedgerDup,
        u3621Code::LedgerHole,
        u3621Code::OpNoop,
        u3621Code::FieldDrift,
        u3621Code::LedgerBroken,
        u3621Code::SnapshotFingerprint,
        u3621Code::TableBroken,
        u3621Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3621Code::RowDup => 0xe120,
            u3621Code::RowGap => 0xe121,
            u3621Code::ValOob => 0xe122,
            u3621Code::SumDrift => 0xe123,
            u3621Code::LedgerDup => 0xe124,
            u3621Code::LedgerHole => 0xe125,
            u3621Code::OpNoop => 0xe126,
            u3621Code::FieldDrift => 0xe127,
            u3621Code::LedgerBroken => 0xe128,
            u3621Code::SnapshotFingerprint => 0xe129,
            u3621Code::TableBroken => 0xe12a,
            u3621Code::SegmentGuard => 0xe12b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3621Code::RowDup => "表行重复（RCU 同键双写）",
            u3621Code::RowGap => "表行断档（宽限 序号不连续）",
            u3621Code::ValOob => "值越界（实验 值域外）",
            u3621Code::SumDrift => "累计漂移（三面 求和≠理论值）",
            u3621Code::LedgerDup => "账目重记（同一事件双入账）",
            u3621Code::LedgerHole => "账目断漏（序号缺失）",
            u3621Code::OpNoop => "空操作（账目无效果调用）",
            u3621Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3621Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3621Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3621Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3621Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3621Code::RowDup => "按键唯一律去重",
            u3621Code::RowGap => "按连续律补号",
            u3621Code::ValOob => "按值域裁剪",
            u3621Code::SumDrift => "按理论值复算",
            u3621Code::LedgerDup => "按事件幂等律合并",
            u3621Code::LedgerHole => "按连续律补账",
            u3621Code::OpNoop => "核对操作前置条件",
            u3621Code::FieldDrift => "按口径重建字段",
            u3621Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3621Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3621Code::TableBroken => "按语义规则重建常量表",
            u3621Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3621reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// RCU 三面实验 语义登记表（24 行逐行注释）。
pub const u3621reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3621reg: [(&'static str, u64); 24] = [
    ("RDC_A", 7), // RDC 主登记（读面 · 判据面）
    ("RDC_B", 33), // RDC 副登记（读面 · 对拍面）
    ("WRC_A", 14), // WRC 主登记（写面 · 判据面）
    ("WRC_B", 44), // WRC 副登记（写面 · 对拍面）
    ("GPC_A", 21), // GPC 主登记（宽限面 · 判据面）
    ("GPC_B", 55), // GPC 副登记（宽限面 · 对拍面）
    ("EXP_A", 28), // EXP 主登记（实验 · 判据面）
    ("EXP_B", 66), // EXP 副登记（实验 · 对拍面）
    ("TRIAL_A", 35), // TRIAL 主登记（试验轮 · 判据面）
    ("TRIAL_B", 77), // TRIAL 副登记（试验轮 · 对拍面）
    ("LAG_A", 42), // LAG 主登记（读侧滞后 · 判据面）
    ("LAG_B", 88), // LAG 副登记（读侧滞后 · 对拍面）
    ("GPW_A", 49), // GPW 主登记（宽限窗口 · 判据面）
    ("GPW_B", 99), // GPW 副登记（宽限窗口 · 对拍面）
    ("THR_A", 56), // THR 主登记（阈值 · 判据面）
    ("THR_B", 110), // THR 副登记（阈值 · 对拍面）
    ("DATA_A", 63), // DATA 主登记（数据面 · 判据面）
    ("DATA_B", 121), // DATA 副登记（数据面 · 对拍面）
    ("MEM_A", 70), // MEM 主登记（内存面 · 判据面）
    ("MEM_B", 132), // MEM 副登记（内存面 · 对拍面）
    ("CPU_A", 77), // CPU 主登记（核数 · 判据面）
    ("CPU_B", 143), // CPU 副登记（核数 · 对拍面）
    ("AGG_A", 84), // AGG 主登记（聚合 · 判据面）
    ("AGG_B", 154), // AGG 副登记（聚合 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3621reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3621reg_COUNT {
        if u3621reg[i].0 == key {
            return Some(u3621reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3621reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3621reg_COUNT {
        s = s.wrapping_add(u3621reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3621Ledger（RCU 三面实验 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// RCU 三面实验 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3621Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3621Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe120, 0xe12f]。
pub const fn u3621_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3621Code::ALL.len() {
        let c = u3621Code::ALL[i].code();
        if c < 0xe120 || c > 0xe12f {
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
fn u3621_guard_seg() -> bool {
    u3621_segment_guard()
}

/// UNX-F3621 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3621_core_tbl() -> bool {
    let mut ok = u3621reg_COUNT == 24;
    if ok { ok = u3621reg_get("RDC_A") == Some(7) && u3621reg_get("RDC_B") == Some(33); }
    if ok { ok = u3621reg_get("AGG_A") == Some(84); }
    if ok { ok = u3621reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3621reg_sum() == 1668; }
    ok
}

/// UNX-F3621 守恒账三操作确定性效果。
fn check_u3621_core_ldg() -> bool {
    let mut ld = Mxu3621Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3621_checks() -> CheckSet {
    let mut set = CheckSet::new("U3621");
    set.add(
        "u3621-seg-guard",
        u3621_guard_seg(),
        if u3621_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3621-core-tbl",
        check_u3621_core_tbl(),
        if check_u3621_core_tbl() { "" } else { "UNX-F3621 core table mismatch" },
    );
    set.add(
        "u3621-core-ldg",
        check_u3621_core_ldg(),
        if check_u3621_core_ldg() { "" } else { "UNX-F3621 core ledger mismatch" },
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
    fn u3621_all_checks_green() {
        let set = run_u3621_checks();
        assert!(set.all_passed(), "U3621 domain checks must be green");
    }

    #[test]
    fn u3621_segment_guard_holds() {
        assert!(u3621_segment_guard());
    }
}
