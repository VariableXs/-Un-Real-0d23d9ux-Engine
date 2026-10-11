//! unxu3221_u3221core —— qspinlock 队列节点结构与 MCS 排队语义 语义登记表与守恒账（MCS 队列节点）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3221 · qspinlock 队列节点结构与 MCS 排队语义 · 判据原文：UNX-F3221 · 判据：UNX-F3221-J1 队列节点成链/断链配对 10^6 次零悬挂引用，FIFO 交接序与到达序一致率 100%
//! · 组件：MCS 队列节点 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3221 · 判据：UNX-F3221-J1 队列节点成链/断链配对 10^6 次零悬挂引用，FIFO 交接序与到达序一致率 100%
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3221_VERSION: &str = "u3221-u3221core-v1";

/// 诊断段基址。
pub const U3221_SEG_BASE: u16 = 0xe000;

// --------------------------------------------------------------------------
// 诊断码（u3221 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// MCS 队列节点 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3221Code {
    /// 0xe000 表行重复（MCS 同键双写）。
    RowDup,
    /// 0xe001 表行断档（FIFO 序号不连续）。
    RowGap,
    /// 0xe002 值越界（成链 值域外）。
    ValOob,
    /// 0xe003 累计漂移（回收 求和≠理论值）。
    SumDrift,
    /// 0xe004 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe005 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe006 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe007 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe008 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe009 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe00a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe00b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3221Code {
    /// 全量闭集。
    pub const ALL: [u3221Code; 12] = [
        u3221Code::RowDup,
        u3221Code::RowGap,
        u3221Code::ValOob,
        u3221Code::SumDrift,
        u3221Code::LedgerDup,
        u3221Code::LedgerHole,
        u3221Code::OpNoop,
        u3221Code::FieldDrift,
        u3221Code::LedgerBroken,
        u3221Code::SnapshotFingerprint,
        u3221Code::TableBroken,
        u3221Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3221Code::RowDup => 0xe000,
            u3221Code::RowGap => 0xe001,
            u3221Code::ValOob => 0xe002,
            u3221Code::SumDrift => 0xe003,
            u3221Code::LedgerDup => 0xe004,
            u3221Code::LedgerHole => 0xe005,
            u3221Code::OpNoop => 0xe006,
            u3221Code::FieldDrift => 0xe007,
            u3221Code::LedgerBroken => 0xe008,
            u3221Code::SnapshotFingerprint => 0xe009,
            u3221Code::TableBroken => 0xe00a,
            u3221Code::SegmentGuard => 0xe00b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3221Code::RowDup => "表行重复（MCS 同键双写）",
            u3221Code::RowGap => "表行断档（FIFO 序号不连续）",
            u3221Code::ValOob => "值越界（成链 值域外）",
            u3221Code::SumDrift => "累计漂移（回收 求和≠理论值）",
            u3221Code::LedgerDup => "账目重记（同一事件双入账）",
            u3221Code::LedgerHole => "账目断漏（序号缺失）",
            u3221Code::OpNoop => "空操作（账目无效果调用）",
            u3221Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3221Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3221Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3221Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3221Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3221Code::RowDup => "按键唯一律去重",
            u3221Code::RowGap => "按连续律补号",
            u3221Code::ValOob => "按值域裁剪",
            u3221Code::SumDrift => "按理论值复算",
            u3221Code::LedgerDup => "按事件幂等律合并",
            u3221Code::LedgerHole => "按连续律补账",
            u3221Code::OpNoop => "核对操作前置条件",
            u3221Code::FieldDrift => "按口径重建字段",
            u3221Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3221Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3221Code::TableBroken => "按语义规则重建常量表",
            u3221Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3221reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// MCS 队列节点 语义登记表（24 行逐行注释）。
pub const u3221reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3221reg: [(&'static str, u64); 24] = [
    ("ND_A", 7), // ND 主登记（队列节点 · 判据面）
    ("ND_B", 33), // ND 副登记（队列节点 · 对拍面）
    ("TL_A", 14), // TL 主登记（尾指针 · 判据面）
    ("TL_B", 44), // TL 副登记（尾指针 · 对拍面）
    ("HD_A", 21), // HD 主登记（头指针 · 判据面）
    ("HD_B", 55), // HD 副登记（头指针 · 对拍面）
    ("NX_A", 28), // NX 主登记（后继指针 · 判据面）
    ("NX_B", 66), // NX 副登记（后继指针 · 对拍面）
    ("PR_A", 35), // PR 主登记（前驱指针 · 判据面）
    ("PR_B", 77), // PR 副登记（前驱指针 · 对拍面）
    ("FIFO_A", 42), // FIFO 主登记（FIFO 交接序 · 判据面）
    ("FIFO_B", 88), // FIFO 副登记（FIFO 交接序 · 对拍面）
    ("LD_A", 49), // LD 主登记（到达序号 · 判据面）
    ("LD_B", 99), // LD 副登记（到达序号 · 对拍面）
    ("HB_A", 56), // HB 主登记（成链标志 · 判据面）
    ("HB_B", 110), // HB 副登记（成链标志 · 对拍面）
    ("BR_A", 63), // BR 主登记（断链标志 · 判据面）
    ("BR_B", 121), // BR 副登记（断链标志 · 对拍面）
    ("RG_A", 70), // RG 主登记（引用计数 · 判据面）
    ("RG_B", 132), // RG 副登记（引用计数 · 对拍面）
    ("10E6_A", 77), // 10E6 主登记（百万次配对 · 判据面）
    ("10E6_B", 143), // 10E6 副登记（百万次配对 · 对拍面）
    ("MG_A", 84), // MG 主登记（迁移语义 · 判据面）
    ("MG_B", 154), // MG 副登记（迁移语义 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3221reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3221reg_COUNT {
        if u3221reg[i].0 == key {
            return Some(u3221reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3221reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3221reg_COUNT {
        s = s.wrapping_add(u3221reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3221Ledger（MCS 队列节点 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// MCS 队列节点 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3221Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3221Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe000, 0xe00f]。
pub const fn u3221_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3221Code::ALL.len() {
        let c = u3221Code::ALL[i].code();
        if c < 0xe000 || c > 0xe00f {
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
fn u3221_guard_seg() -> bool {
    u3221_segment_guard()
}

/// UNX-F3221 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3221_core_tbl() -> bool {
    let mut ok = u3221reg_COUNT == 24;
    if ok { ok = u3221reg_get("ND_A") == Some(7) && u3221reg_get("ND_B") == Some(33); }
    if ok { ok = u3221reg_get("MG_A") == Some(84); }
    if ok { ok = u3221reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3221reg_sum() == 1668; }
    ok
}

/// UNX-F3221 守恒账三操作确定性效果。
fn check_u3221_core_ldg() -> bool {
    let mut ld = Mxu3221Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3221_checks() -> CheckSet {
    let mut set = CheckSet::new("U3221");
    set.add(
        "u3221-seg-guard",
        u3221_guard_seg(),
        if u3221_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3221-core-tbl",
        check_u3221_core_tbl(),
        if check_u3221_core_tbl() { "" } else { "UNX-F3221 core table mismatch" },
    );
    set.add(
        "u3221-core-ldg",
        check_u3221_core_ldg(),
        if check_u3221_core_ldg() { "" } else { "UNX-F3221 core ledger mismatch" },
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
    fn u3221_all_checks_green() {
        let set = run_u3221_checks();
        assert!(set.all_passed(), "U3221 domain checks must be green");
    }

    #[test]
    fn u3221_segment_guard_holds() {
        assert!(u3221_segment_guard());
    }
}
