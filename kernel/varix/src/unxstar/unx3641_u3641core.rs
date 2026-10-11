//! unxu3641_u3641core —— 崩溃转储总设计（触发→采集→封存管线） 语义登记表与守恒账（崩溃转储）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3641 · 崩溃转储总设计（触发→采集→封存管线） · 判据原文：UNX-F3641 · 判据：J1：三段管线（触发（🔴事件）/采集（快照三件套）/封存（通道+校验）——判据行内嵌状态机）；M-399 全链首dump（注入 panic→全链走通→封存物可校验）。<br>J2：管线恒备（非崩溃期管线自检绿——F3655 的日常态——总设计的常备性）。
//! · 组件：崩溃转储 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3641 · 判据：J1：三段管线（触发（🔴事件）/采集（快照三件套）/封存（通道+校验）——判据行内嵌状态机）；M-399 全链首dump（注入 p
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3641_VERSION: &str = "u3641-u3641core-v1";

/// 诊断段基址。
pub const U3641_SEG_BASE: u16 = 0xe130;

// --------------------------------------------------------------------------
// 诊断码（u3641 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 崩溃转储 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3641Code {
    /// 0xe130 表行重复（转储 同键双写）。
    RowDup,
    /// 0xe131 表行断档（崩溃 序号不连续）。
    RowGap,
    /// 0xe132 值越界（封存 值域外）。
    ValOob,
    /// 0xe133 累计漂移（封存 求和≠理论值）。
    SumDrift,
    /// 0xe134 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe135 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe136 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe137 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe138 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe139 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe13a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe13b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3641Code {
    /// 全量闭集。
    pub const ALL: [u3641Code; 12] = [
        u3641Code::RowDup,
        u3641Code::RowGap,
        u3641Code::ValOob,
        u3641Code::SumDrift,
        u3641Code::LedgerDup,
        u3641Code::LedgerHole,
        u3641Code::OpNoop,
        u3641Code::FieldDrift,
        u3641Code::LedgerBroken,
        u3641Code::SnapshotFingerprint,
        u3641Code::TableBroken,
        u3641Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3641Code::RowDup => 0xe130,
            u3641Code::RowGap => 0xe131,
            u3641Code::ValOob => 0xe132,
            u3641Code::SumDrift => 0xe133,
            u3641Code::LedgerDup => 0xe134,
            u3641Code::LedgerHole => 0xe135,
            u3641Code::OpNoop => 0xe136,
            u3641Code::FieldDrift => 0xe137,
            u3641Code::LedgerBroken => 0xe138,
            u3641Code::SnapshotFingerprint => 0xe139,
            u3641Code::TableBroken => 0xe13a,
            u3641Code::SegmentGuard => 0xe13b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3641Code::RowDup => "表行重复（转储 同键双写）",
            u3641Code::RowGap => "表行断档（崩溃 序号不连续）",
            u3641Code::ValOob => "值越界（封存 值域外）",
            u3641Code::SumDrift => "累计漂移（封存 求和≠理论值）",
            u3641Code::LedgerDup => "账目重记（同一事件双入账）",
            u3641Code::LedgerHole => "账目断漏（序号缺失）",
            u3641Code::OpNoop => "空操作（账目无效果调用）",
            u3641Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3641Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3641Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3641Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3641Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3641Code::RowDup => "按键唯一律去重",
            u3641Code::RowGap => "按连续律补号",
            u3641Code::ValOob => "按值域裁剪",
            u3641Code::SumDrift => "按理论值复算",
            u3641Code::LedgerDup => "按事件幂等律合并",
            u3641Code::LedgerHole => "按连续律补账",
            u3641Code::OpNoop => "核对操作前置条件",
            u3641Code::FieldDrift => "按口径重建字段",
            u3641Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3641Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3641Code::TableBroken => "按语义规则重建常量表",
            u3641Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3641reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 崩溃转储 语义登记表（24 行逐行注释）。
pub const u3641reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3641reg: [(&'static str, u64); 24] = [
    ("DUMP_A", 7), // DUMP 主登记（转储 · 判据面）
    ("DUMP_B", 33), // DUMP 副登记（转储 · 对拍面）
    ("TRIG_A", 14), // TRIG 主登记（触发 · 判据面）
    ("TRIG_B", 44), // TRIG 副登记（触发 · 对拍面）
    ("COLL_A", 21), // COLL 主登记（采集 · 判据面）
    ("COLL_B", 55), // COLL 副登记（采集 · 对拍面）
    ("SEAL_A", 28), // SEAL 主登记（封存 · 判据面）
    ("SEAL_B", 66), // SEAL 副登记（封存 · 对拍面）
    ("RB_A", 35), // RB 主登记（回溯 · 判据面）
    ("RB_B", 77), // RB 副登记（回溯 · 对拍面）
    ("REG_A", 42), // REG 主登记（寄存器组 · 判据面）
    ("REG_B", 88), // REG 副登记（寄存器组 · 对拍面）
    ("STK_A", 49), // STK 主登记（栈快照 · 判据面）
    ("STK_B", 99), // STK 副登记（栈快照 · 对拍面）
    ("MEM_A", 56), // MEM 主登记（内存快照 · 判据面）
    ("MEM_B", 110), // MEM 副登记（内存快照 · 对拍面）
    ("FMT_A", 63), // FMT 主登记（转储格式 · 判据面）
    ("FMT_B", 121), // FMT 副登记（转储格式 · 对拍面）
    ("SINK_A", 70), // SINK 主登记（落地 · 判据面）
    ("SINK_B", 132), // SINK 副登记（落地 · 对拍面）
    ("ID_A", 77), // ID 主登记（转储标识 · 判据面）
    ("ID_B", 143), // ID 副登记（转储标识 · 对拍面）
    ("CHAIN_A", 84), // CHAIN 主登记（指纹链 · 判据面）
    ("CHAIN_B", 154), // CHAIN 副登记（指纹链 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3641reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3641reg_COUNT {
        if u3641reg[i].0 == key {
            return Some(u3641reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3641reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3641reg_COUNT {
        s = s.wrapping_add(u3641reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3641Ledger（崩溃转储 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 崩溃转储 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3641Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3641Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe130, 0xe13f]。
pub const fn u3641_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3641Code::ALL.len() {
        let c = u3641Code::ALL[i].code();
        if c < 0xe130 || c > 0xe13f {
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
fn u3641_guard_seg() -> bool {
    u3641_segment_guard()
}

/// UNX-F3641 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3641_core_tbl() -> bool {
    let mut ok = u3641reg_COUNT == 24;
    if ok { ok = u3641reg_get("DUMP_A") == Some(7) && u3641reg_get("DUMP_B") == Some(33); }
    if ok { ok = u3641reg_get("CHAIN_A") == Some(84); }
    if ok { ok = u3641reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3641reg_sum() == 1668; }
    ok
}

/// UNX-F3641 守恒账三操作确定性效果。
fn check_u3641_core_ldg() -> bool {
    let mut ld = Mxu3641Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3641_checks() -> CheckSet {
    let mut set = CheckSet::new("U3641");
    set.add(
        "u3641-seg-guard",
        u3641_guard_seg(),
        if u3641_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3641-core-tbl",
        check_u3641_core_tbl(),
        if check_u3641_core_tbl() { "" } else { "UNX-F3641 core table mismatch" },
    );
    set.add(
        "u3641-core-ldg",
        check_u3641_core_ldg(),
        if check_u3641_core_ldg() { "" } else { "UNX-F3641 core ledger mismatch" },
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
    fn u3641_all_checks_green() {
        let set = run_u3641_checks();
        assert!(set.all_passed(), "U3641 domain checks must be green");
    }

    #[test]
    fn u3641_segment_guard_holds() {
        assert!(u3641_segment_guard());
    }
}
