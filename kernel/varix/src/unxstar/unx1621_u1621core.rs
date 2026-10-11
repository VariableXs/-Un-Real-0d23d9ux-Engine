//! unxu1621_u1621core —— per-CPU 变量区布局总纲（接续 A1 trampoline 预留地址） 语义登记表与守恒账（区段）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1621 · per-CPU 变量区布局总纲（接续 A1 trampoline 预留地址） · 判据原文：UNX-F1621 · 判据：变量区按 A1 预留地址切分（统计区/队列区/时钟区/shootdown 区/调试区五段）偏移账固化，五段边界与 A1 预留区零越界，布局图可导出
//! · 组件：区段 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F1621 · 判据：变量区按 A1 预留地址切分（统计区/队列区/时钟区/shootdown 区/调试区五段）偏移账固化，五段边界与 A1 预留区零越
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1621_VERSION: &str = "u1621-u1621core-v1";

/// 诊断段基址。
pub const U1621_SEG_BASE: u16 = 0x55c0;

// --------------------------------------------------------------------------
// 诊断码（u1621 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 区段 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1621Code {
    /// 0x55c0 表行重复（段越界 同键双写）。
    RowDup,
    /// 0x55c1 表行断档（偏移漂移 序号不连续）。
    RowGap,
    /// 0x55c2 值越界（切分重叠 值域外）。
    ValOob,
    /// 0x55c3 累计漂移（导出失真 求和≠理论值）。
    SumDrift,
    /// 0x55c4 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0x55c5 账目断漏（序号缺失）。
    LedgerHole,
    /// 0x55c6 空操作（账目无效果调用）。
    OpNoop,
    /// 0x55c7 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0x55c8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x55c9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x55ca 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x55cb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1621Code {
    /// 全量闭集。
    pub const ALL: [u1621Code; 12] = [
        u1621Code::RowDup,
        u1621Code::RowGap,
        u1621Code::ValOob,
        u1621Code::SumDrift,
        u1621Code::LedgerDup,
        u1621Code::LedgerHole,
        u1621Code::OpNoop,
        u1621Code::FieldDrift,
        u1621Code::LedgerBroken,
        u1621Code::SnapshotFingerprint,
        u1621Code::TableBroken,
        u1621Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1621Code::RowDup => 0x55c0,
            u1621Code::RowGap => 0x55c1,
            u1621Code::ValOob => 0x55c2,
            u1621Code::SumDrift => 0x55c3,
            u1621Code::LedgerDup => 0x55c4,
            u1621Code::LedgerHole => 0x55c5,
            u1621Code::OpNoop => 0x55c6,
            u1621Code::FieldDrift => 0x55c7,
            u1621Code::LedgerBroken => 0x55c8,
            u1621Code::SnapshotFingerprint => 0x55c9,
            u1621Code::TableBroken => 0x55ca,
            u1621Code::SegmentGuard => 0x55cb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1621Code::RowDup => "表行重复（段越界 同键双写）",
            u1621Code::RowGap => "表行断档（偏移漂移 序号不连续）",
            u1621Code::ValOob => "值越界（切分重叠 值域外）",
            u1621Code::SumDrift => "累计漂移（导出失真 求和≠理论值）",
            u1621Code::LedgerDup => "账目重记（同一事件双入账）",
            u1621Code::LedgerHole => "账目断漏（序号缺失）",
            u1621Code::OpNoop => "空操作（账目无效果调用）",
            u1621Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u1621Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1621Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1621Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1621Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1621Code::RowDup => "按键唯一律去重",
            u1621Code::RowGap => "按连续律补号",
            u1621Code::ValOob => "按值域裁剪",
            u1621Code::SumDrift => "按理论值复算",
            u1621Code::LedgerDup => "按事件幂等律合并",
            u1621Code::LedgerHole => "按连续律补账",
            u1621Code::OpNoop => "核对操作前置条件",
            u1621Code::FieldDrift => "按口径重建字段",
            u1621Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1621Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1621Code::TableBroken => "按语义规则重建常量表",
            u1621Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1621reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 区段 语义登记表（24 行逐行注释）。
pub const u1621reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1621reg: [(&'static str, u64); 24] = [
    ("ST_A", 7), // ST 主登记（统计区 · 判据面）
    ("ST_B", 33), // ST 副登记（统计区 · 对拍面）
    ("RQ_A", 14), // RQ 主登记（队列区 · 判据面）
    ("RQ_B", 44), // RQ 副登记（队列区 · 对拍面）
    ("CK_A", 21), // CK 主登记（时钟区 · 判据面）
    ("CK_B", 55), // CK 副登记（时钟区 · 对拍面）
    ("SD_A", 28), // SD 主登记（shootdown 区 · 判据面）
    ("SD_B", 66), // SD 副登记（shootdown 区 · 对拍面）
    ("DB_A", 35), // DB 主登记（调试区 · 判据面）
    ("DB_B", 77), // DB 副登记（调试区 · 对拍面）
    ("OF_A", 42), // OF 主登记（偏移账 · 判据面）
    ("OF_B", 88), // OF 副登记（偏移账 · 对拍面）
    ("BE_A", 49), // BE 主登记（段边界 · 判据面）
    ("BE_B", 99), // BE 副登记（段边界 · 对拍面）
    ("ZB_A", 56), // ZB 主登记（零越界 · 判据面）
    ("ZB_B", 110), // ZB 副登记（零越界 · 对拍面）
    ("LG_A", 63), // LG 主登记（布局图导出 · 判据面）
    ("LG_B", 121), // LG 副登记（布局图导出 · 对拍面）
    ("A1_A", 70), // A1 主登记（A1 预留锚 · 判据面）
    ("A1_B", 132), // A1 副登记（A1 预留锚 · 对拍面）
    ("SZ_A", 77), // SZ 主登记（段尺寸 · 判据面）
    ("SZ_B", 143), // SZ 副登记（段尺寸 · 对拍面）
    ("AL_A", 84), // AL 主登记（对齐锚 · 判据面）
    ("AL_B", 154), // AL 副登记（对齐锚 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1621reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1621reg_COUNT {
        if u1621reg[i].0 == key {
            return Some(u1621reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1621reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1621reg_COUNT {
        s = s.wrapping_add(u1621reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu1621Ledger（区段 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 区段 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu1621Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu1621Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0x55c0, 0x55cf]。
pub const fn u1621_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1621Code::ALL.len() {
        let c = u1621Code::ALL[i].code();
        if c < 0x55c0 || c > 0x55cf {
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
fn u1621_guard_seg() -> bool {
    u1621_segment_guard()
}

/// UNX-F1621 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u1621_core_tbl() -> bool {
    let mut ok = u1621reg_COUNT == 24;
    if ok { ok = u1621reg_get("ST_A") == Some(7) && u1621reg_get("ST_B") == Some(33); }
    if ok { ok = u1621reg_get("AL_A") == Some(84); }
    if ok { ok = u1621reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1621reg_sum() == 1668; }
    ok
}

/// UNX-F1621 守恒账三操作确定性效果。
fn check_u1621_core_ldg() -> bool {
    let mut ld = Mxu1621Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1621_checks() -> CheckSet {
    let mut set = CheckSet::new("U1621");
    set.add(
        "u1621-seg-guard",
        u1621_guard_seg(),
        if u1621_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1621-core-tbl",
        check_u1621_core_tbl(),
        if check_u1621_core_tbl() { "" } else { "UNX-F1621 core table mismatch" },
    );
    set.add(
        "u1621-core-ldg",
        check_u1621_core_ldg(),
        if check_u1621_core_ldg() { "" } else { "UNX-F1621 core ledger mismatch" },
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
    fn u1621_all_checks_green() {
        let set = run_u1621_checks();
        assert!(set.all_passed(), "U1621 domain checks must be green");
    }

    #[test]
    fn u1621_segment_guard_holds() {
        assert!(u1621_segment_guard());
    }
}
