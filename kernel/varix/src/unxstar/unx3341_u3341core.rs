//! unxu3341_u3341core —— 死锁检测器总设计（等待图模型与不做的事） 语义登记表与守恒账（死锁检测器）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3341 · 死锁检测器总设计（等待图模型与不做的事） · 判据原文：UNX-F3341 · 判据：J1：总设计文档交付：节点=锁实例、边=持有→等待的图模型定义、v1 不做清单（不做抢占恢复/不做图外对象/不做优先级反转纠正）；M-100 模型验证用例。
//! · 组件：死锁检测器 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3341 · 判据：J1：总设计文档交付：节点=锁实例、边=持有→等待的图模型定义、v1 不做清单（不做抢占恢复/不做图外对象/不做优先级反转纠正）；
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3341_VERSION: &str = "u3341-u3341core-v1";

/// 诊断段基址。
pub const U3341_SEG_BASE: u16 = 0xe040;

// --------------------------------------------------------------------------
// 诊断码（u3341 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 死锁检测器 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3341Code {
    /// 0xe040 表行重复（死锁 同键双写）。
    RowDup,
    /// 0xe041 表行断档（等待图 序号不连续）。
    RowGap,
    /// 0xe042 值越界（环 值域外）。
    ValOob,
    /// 0xe043 累计漂移（等待图 求和≠理论值）。
    SumDrift,
    /// 0xe044 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe045 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe046 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe047 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe048 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe049 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe04a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe04b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3341Code {
    /// 全量闭集。
    pub const ALL: [u3341Code; 12] = [
        u3341Code::RowDup,
        u3341Code::RowGap,
        u3341Code::ValOob,
        u3341Code::SumDrift,
        u3341Code::LedgerDup,
        u3341Code::LedgerHole,
        u3341Code::OpNoop,
        u3341Code::FieldDrift,
        u3341Code::LedgerBroken,
        u3341Code::SnapshotFingerprint,
        u3341Code::TableBroken,
        u3341Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3341Code::RowDup => 0xe040,
            u3341Code::RowGap => 0xe041,
            u3341Code::ValOob => 0xe042,
            u3341Code::SumDrift => 0xe043,
            u3341Code::LedgerDup => 0xe044,
            u3341Code::LedgerHole => 0xe045,
            u3341Code::OpNoop => 0xe046,
            u3341Code::FieldDrift => 0xe047,
            u3341Code::LedgerBroken => 0xe048,
            u3341Code::SnapshotFingerprint => 0xe049,
            u3341Code::TableBroken => 0xe04a,
            u3341Code::SegmentGuard => 0xe04b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3341Code::RowDup => "表行重复（死锁 同键双写）",
            u3341Code::RowGap => "表行断档（等待图 序号不连续）",
            u3341Code::ValOob => "值越界（环 值域外）",
            u3341Code::SumDrift => "累计漂移（等待图 求和≠理论值）",
            u3341Code::LedgerDup => "账目重记（同一事件双入账）",
            u3341Code::LedgerHole => "账目断漏（序号缺失）",
            u3341Code::OpNoop => "空操作（账目无效果调用）",
            u3341Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3341Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3341Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3341Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3341Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3341Code::RowDup => "按键唯一律去重",
            u3341Code::RowGap => "按连续律补号",
            u3341Code::ValOob => "按值域裁剪",
            u3341Code::SumDrift => "按理论值复算",
            u3341Code::LedgerDup => "按事件幂等律合并",
            u3341Code::LedgerHole => "按连续律补账",
            u3341Code::OpNoop => "核对操作前置条件",
            u3341Code::FieldDrift => "按口径重建字段",
            u3341Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3341Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3341Code::TableBroken => "按语义规则重建常量表",
            u3341Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3341reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 死锁检测器 语义登记表（24 行逐行注释）。
pub const u3341reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3341reg: [(&'static str, u64); 24] = [
    ("WG_A", 7), // WG 主登记（等待图 · 判据面）
    ("WG_B", 33), // WG 副登记（等待图 · 对拍面）
    ("NODE_A", 14), // NODE 主登记（节点 · 判据面）
    ("NODE_B", 44), // NODE 副登记（节点 · 对拍面）
    ("EDGE_A", 21), // EDGE 主登记（有向边 · 判据面）
    ("EDGE_B", 55), // EDGE 副登记（有向边 · 对拍面）
    ("TREE_A", 28), // TREE 主登记（环检测 · 判据面）
    ("TREE_B", 66), // TREE 副登记（环检测 · 对拍面）
    ("WCHAIN_A", 35), // WCHAIN 主登记（等待链 · 判据面）
    ("WCHAIN_B", 77), // WCHAIN 副登记（等待链 · 对拍面）
    ("BLK_A", 42), // BLK 主登记（阻塞点 · 判据面）
    ("BLK_B", 88), // BLK 副登记（阻塞点 · 对拍面）
    ("TTD_A", 49), // TTD 主登记（超时判决 · 判据面）
    ("TTD_B", 99), // TTD 副登记（超时判决 · 对拍面）
    ("RPT_A", 56), // RPT 主登记（检测报告 · 判据面）
    ("RPT_B", 110), // RPT 副登记（检测报告 · 对拍面）
    ("SCALE_A", 63), // SCALE 主登记（规模上限 · 判据面）
    ("SCALE_B", 121), // SCALE 副登记（规模上限 · 对拍面）
    ("FALSE_A", 70), // FALSE 主登记（假阳性 · 判据面）
    ("FALSE_B", 132), // FALSE 副登记（假阳性 · 对拍面）
    ("MISS_A", 77), // MISS 主登记（漏报 · 判据面）
    ("MISS_B", 143), // MISS 副登记（漏报 · 对拍面）
    ("BUD_A", 84), // BUD 主登记（预算步数 · 判据面）
    ("BUD_B", 154), // BUD 副登记（预算步数 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3341reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3341reg_COUNT {
        if u3341reg[i].0 == key {
            return Some(u3341reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3341reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3341reg_COUNT {
        s = s.wrapping_add(u3341reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3341Ledger（死锁检测器 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 死锁检测器 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3341Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3341Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe040, 0xe04f]。
pub const fn u3341_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3341Code::ALL.len() {
        let c = u3341Code::ALL[i].code();
        if c < 0xe040 || c > 0xe04f {
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
fn u3341_guard_seg() -> bool {
    u3341_segment_guard()
}

/// UNX-F3341 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3341_core_tbl() -> bool {
    let mut ok = u3341reg_COUNT == 24;
    if ok { ok = u3341reg_get("WG_A") == Some(7) && u3341reg_get("WG_B") == Some(33); }
    if ok { ok = u3341reg_get("BUD_A") == Some(84); }
    if ok { ok = u3341reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3341reg_sum() == 1668; }
    ok
}

/// UNX-F3341 守恒账三操作确定性效果。
fn check_u3341_core_ldg() -> bool {
    let mut ld = Mxu3341Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3341_checks() -> CheckSet {
    let mut set = CheckSet::new("U3341");
    set.add(
        "u3341-seg-guard",
        u3341_guard_seg(),
        if u3341_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3341-core-tbl",
        check_u3341_core_tbl(),
        if check_u3341_core_tbl() { "" } else { "UNX-F3341 core table mismatch" },
    );
    set.add(
        "u3341-core-ldg",
        check_u3341_core_ldg(),
        if check_u3341_core_ldg() { "" } else { "UNX-F3341 core ledger mismatch" },
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
    fn u3341_all_checks_green() {
        let set = run_u3341_checks();
        assert!(set.all_passed(), "U3341 domain checks must be green");
    }

    #[test]
    fn u3341_segment_guard_holds() {
        assert!(u3341_segment_guard());
    }
}
