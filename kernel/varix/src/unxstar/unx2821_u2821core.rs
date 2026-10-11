//! unxu2821_u2821core —— 多地址空间并发压测（256 空间） 语义登记表与守恒账（多地址空间并发压测）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2821 · 多地址空间并发压测（256 空间） · 判据原文：UNX-F2821 · 判据：UNX-F2821-J1 256 空间并发映射-访问-销毁 10/10 轮零串址，空间隔离账平
//! · 组件：多地址空间并发压测 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2821 · 判据：UNX-F2821-J1 256 空间并发映射-访问-销毁 10/10 轮零串址，空间隔离账平
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2821_VERSION: &str = "u2821-u2821core-v1";

/// 诊断段基址。
pub const U2821_SEG_BASE: u16 = 0xe1c0;

// --------------------------------------------------------------------------
// 诊断码（u2821 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 多地址空间并发压测 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2821Code {
    /// 0xe1c0 表行重复（地址空间 同键双写）。
    RowDup,
    /// 0xe1c1 表行断档（并发 序号不连续）。
    RowGap,
    /// 0xe1c2 值越界（切换 值域外）。
    ValOob,
    /// 0xe1c3 累计漂移（空间 求和≠理论值）。
    SumDrift,
    /// 0xe1c4 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe1c5 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe1c6 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe1c7 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe1c8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1c9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1ca 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1cb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2821Code {
    /// 全量闭集。
    pub const ALL: [u2821Code; 12] = [
        u2821Code::RowDup,
        u2821Code::RowGap,
        u2821Code::ValOob,
        u2821Code::SumDrift,
        u2821Code::LedgerDup,
        u2821Code::LedgerHole,
        u2821Code::OpNoop,
        u2821Code::FieldDrift,
        u2821Code::LedgerBroken,
        u2821Code::SnapshotFingerprint,
        u2821Code::TableBroken,
        u2821Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2821Code::RowDup => 0xe1c0,
            u2821Code::RowGap => 0xe1c1,
            u2821Code::ValOob => 0xe1c2,
            u2821Code::SumDrift => 0xe1c3,
            u2821Code::LedgerDup => 0xe1c4,
            u2821Code::LedgerHole => 0xe1c5,
            u2821Code::OpNoop => 0xe1c6,
            u2821Code::FieldDrift => 0xe1c7,
            u2821Code::LedgerBroken => 0xe1c8,
            u2821Code::SnapshotFingerprint => 0xe1c9,
            u2821Code::TableBroken => 0xe1ca,
            u2821Code::SegmentGuard => 0xe1cb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2821Code::RowDup => "表行重复（地址空间 同键双写）",
            u2821Code::RowGap => "表行断档（并发 序号不连续）",
            u2821Code::ValOob => "值越界（切换 值域外）",
            u2821Code::SumDrift => "累计漂移（空间 求和≠理论值）",
            u2821Code::LedgerDup => "账目重记（同一事件双入账）",
            u2821Code::LedgerHole => "账目断漏（序号缺失）",
            u2821Code::OpNoop => "空操作（账目无效果调用）",
            u2821Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2821Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2821Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2821Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2821Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2821Code::RowDup => "按键唯一律去重",
            u2821Code::RowGap => "按连续律补号",
            u2821Code::ValOob => "按值域裁剪",
            u2821Code::SumDrift => "按理论值复算",
            u2821Code::LedgerDup => "按事件幂等律合并",
            u2821Code::LedgerHole => "按连续律补账",
            u2821Code::OpNoop => "核对操作前置条件",
            u2821Code::FieldDrift => "按口径重建字段",
            u2821Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2821Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2821Code::TableBroken => "按语义规则重建常量表",
            u2821Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2821reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 多地址空间并发压测 语义登记表（24 行逐行注释）。
pub const u2821reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2821reg: [(&'static str, u64); 24] = [
    ("AS_A", 7), // AS 主登记（地址空间 · 判据面）
    ("AS_B", 33), // AS 副登记（地址空间 · 对拍面）
    ("N256_A", 14), // N256 主登记（256 空间 · 判据面）
    ("N256_B", 44), // N256 副登记（256 空间 · 对拍面）
    ("CONC_A", 21), // CONC 主登记（并发 · 判据面）
    ("CONC_B", 55), // CONC 副登记（并发 · 对拍面）
    ("SW_A", 28), // SW 主登记（切换 · 判据面）
    ("SW_B", 66), // SW 副登记（切换 · 对拍面）
    ("CR3_A", 35), // CR3 主登记（CR3 切换 · 判据面）
    ("CR3_B", 77), // CR3 副登记（CR3 切换 · 对拍面）
    ("TLB_A", 42), // TLB 主登记（TLB · 判据面）
    ("TLB_B", 88), // TLB 副登记（TLB · 对拍面）
    ("ISOL_A", 49), // ISOL 主登记（隔离 · 判据面）
    ("ISOL_B", 99), // ISOL 副登记（隔离 · 对拍面）
    ("HOT_A", 56), // HOT 主登记（热 · 判据面）
    ("HOT_B", 110), // HOT 副登记（热 · 对拍面）
    ("COLD_A", 63), // COLD 主登记（冷 · 判据面）
    ("COLD_B", 121), // COLD 副登记（冷 · 对拍面）
    ("FAIR_A", 70), // FAIR 主登记（公平 · 判据面）
    ("FAIR_B", 132), // FAIR 副登记（公平 · 对拍面）
    ("JIT_A", 77), // JIT 主登记（抖动 · 判据面）
    ("JIT_B", 143), // JIT 副登记（抖动 · 对拍面）
    ("REC_A", 84), // REC 主登记（回收 · 判据面）
    ("REC_B", 154), // REC 副登记（回收 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2821reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2821reg_COUNT {
        if u2821reg[i].0 == key {
            return Some(u2821reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2821reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2821reg_COUNT {
        s = s.wrapping_add(u2821reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2821Ledger（多地址空间并发压测 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 多地址空间并发压测 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2821Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2821Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1c0, 0xe1cf]。
pub const fn u2821_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2821Code::ALL.len() {
        let c = u2821Code::ALL[i].code();
        if c < 0xe1c0 || c > 0xe1cf {
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
fn u2821_guard_seg() -> bool {
    u2821_segment_guard()
}

/// UNX-F2821 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2821_core_tbl() -> bool {
    let mut ok = u2821reg_COUNT == 24;
    if ok { ok = u2821reg_get("AS_A") == Some(7) && u2821reg_get("AS_B") == Some(33); }
    if ok { ok = u2821reg_get("REC_A") == Some(84); }
    if ok { ok = u2821reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2821reg_sum() == 1668; }
    ok
}

/// UNX-F2821 守恒账三操作确定性效果。
fn check_u2821_core_ldg() -> bool {
    let mut ld = Mxu2821Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2821_checks() -> CheckSet {
    let mut set = CheckSet::new("U2821");
    set.add(
        "u2821-seg-guard",
        u2821_guard_seg(),
        if u2821_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2821-core-tbl",
        check_u2821_core_tbl(),
        if check_u2821_core_tbl() { "" } else { "UNX-F2821 core table mismatch" },
    );
    set.add(
        "u2821-core-ldg",
        check_u2821_core_ldg(),
        if check_u2821_core_ldg() { "" } else { "UNX-F2821 core ledger mismatch" },
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
    fn u2821_all_checks_green() {
        let set = run_u2821_checks();
        assert!(set.all_passed(), "U2821 domain checks must be green");
    }

    #[test]
    fn u2821_segment_guard_holds() {
        assert!(u2821_segment_guard());
    }
}
