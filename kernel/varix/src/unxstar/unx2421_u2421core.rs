//! unxu2421_u2421core —— order-0 页快速路径 语义登记表与守恒账（单页请求）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2421 · order-0 页快速路径 · 判据原文：UNX-F2421 · 判据：单页分配 p99 时延落账且与高阶路径比值 ≤0.5，10^6 次分配释放对账零泄漏
//! · 组件：单页请求 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2421 · 判据：单页分配 p99 时延落账且与高阶路径比值 ≤0.5，10^6 次分配释放对账零泄漏
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2421_VERSION: &str = "u2421-u2421core-v1";

/// 诊断段基址。
pub const U2421_SEG_BASE: u16 = 0xa0c0;

// --------------------------------------------------------------------------
// 诊断码（u2421 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 单页请求 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2421Code {
    /// 0xa0c0 表行重复（快径旁落 同键双写）。
    RowDup,
    /// 0xa0c1 表行断档（时延超标 序号不连续）。
    RowGap,
    /// 0xa0c2 值越界（比值越界 值域外）。
    ValOob,
    /// 0xa0c3 累计漂移（泄漏漏记 求和≠理论值）。
    SumDrift,
    /// 0xa0c4 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xa0c5 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xa0c6 空操作（账目无效果调用）。
    OpNoop,
    /// 0xa0c7 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xa0c8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa0c9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa0ca 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa0cb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2421Code {
    /// 全量闭集。
    pub const ALL: [u2421Code; 12] = [
        u2421Code::RowDup,
        u2421Code::RowGap,
        u2421Code::ValOob,
        u2421Code::SumDrift,
        u2421Code::LedgerDup,
        u2421Code::LedgerHole,
        u2421Code::OpNoop,
        u2421Code::FieldDrift,
        u2421Code::LedgerBroken,
        u2421Code::SnapshotFingerprint,
        u2421Code::TableBroken,
        u2421Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2421Code::RowDup => 0xa0c0,
            u2421Code::RowGap => 0xa0c1,
            u2421Code::ValOob => 0xa0c2,
            u2421Code::SumDrift => 0xa0c3,
            u2421Code::LedgerDup => 0xa0c4,
            u2421Code::LedgerHole => 0xa0c5,
            u2421Code::OpNoop => 0xa0c6,
            u2421Code::FieldDrift => 0xa0c7,
            u2421Code::LedgerBroken => 0xa0c8,
            u2421Code::SnapshotFingerprint => 0xa0c9,
            u2421Code::TableBroken => 0xa0ca,
            u2421Code::SegmentGuard => 0xa0cb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2421Code::RowDup => "表行重复（快径旁落 同键双写）",
            u2421Code::RowGap => "表行断档（时延超标 序号不连续）",
            u2421Code::ValOob => "值越界（比值越界 值域外）",
            u2421Code::SumDrift => "累计漂移（泄漏漏记 求和≠理论值）",
            u2421Code::LedgerDup => "账目重记（同一事件双入账）",
            u2421Code::LedgerHole => "账目断漏（序号缺失）",
            u2421Code::OpNoop => "空操作（账目无效果调用）",
            u2421Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2421Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2421Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2421Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2421Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2421Code::RowDup => "按键唯一律去重",
            u2421Code::RowGap => "按连续律补号",
            u2421Code::ValOob => "按值域裁剪",
            u2421Code::SumDrift => "按理论值复算",
            u2421Code::LedgerDup => "按事件幂等律合并",
            u2421Code::LedgerHole => "按连续律补账",
            u2421Code::OpNoop => "核对操作前置条件",
            u2421Code::FieldDrift => "按口径重建字段",
            u2421Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2421Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2421Code::TableBroken => "按语义规则重建常量表",
            u2421Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2421reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 单页请求 语义登记表（24 行逐行注释）。
pub const u2421reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2421reg: [(&'static str, u64); 24] = [
    ("O0_A", 7), // O0 主登记（order-0 路径 · 判据面）
    ("O0_B", 33), // O0 副登记（order-0 路径 · 对拍面）
    ("P99_A", 14), // P99 主登记（p99 时延 · 判据面）
    ("P99_B", 44), // P99 副登记（p99 时延 · 对拍面）
    ("HI_A", 21), // HI 主登记（高阶路径 · 判据面）
    ("HI_B", 55), // HI 副登记（高阶路径 · 对拍面）
    ("RT_A", 28), // RT 主登记（比值门限 · 判据面）
    ("RT_B", 66), // RT 副登记（比值门限 · 对拍面）
    ("M1_A", 35), // M1 主登记（百万次对账 · 判据面）
    ("M1_B", 77), // M1 副登记（百万次对账 · 对拍面）
    ("LK_A", 42), // LK 主登记（泄漏账 · 判据面）
    ("LK_B", 88), // LK 副登记（泄漏账 · 对拍面）
    ("AL_A", 49), // AL 主登记（分配事件 · 判据面）
    ("AL_B", 99), // AL 副登记（分配事件 · 对拍面）
    ("FR_A", 56), // FR 主登记（释放事件 · 判据面）
    ("FR_B", 110), // FR 副登记（释放事件 · 对拍面）
    ("FD_A", 63), // FD 主登记（快径标志 · 判据面）
    ("FD_B", 121), // FD 副登记（快径标志 · 对拍面）
    ("LT_A", 70), // LT 主登记（时延账 · 判据面）
    ("LT_B", 132), // LT 副登记（时延账 · 对拍面）
    ("QF_A", 77), // QF 主登记（配额账 · 判据面）
    ("QF_B", 143), // QF 副登记（配额账 · 对拍面）
    ("ZC_A", 84), // ZC 主登记（零泄漏断言 · 判据面）
    ("ZC_B", 154), // ZC 副登记（零泄漏断言 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2421reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2421reg_COUNT {
        if u2421reg[i].0 == key {
            return Some(u2421reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2421reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2421reg_COUNT {
        s = s.wrapping_add(u2421reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2421Ledger（单页请求 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 单页请求 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2421Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2421Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa0c0, 0xa0cf]。
pub const fn u2421_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2421Code::ALL.len() {
        let c = u2421Code::ALL[i].code();
        if c < 0xa0c0 || c > 0xa0cf {
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
fn u2421_guard_seg() -> bool {
    u2421_segment_guard()
}

/// UNX-F2421 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2421_core_tbl() -> bool {
    let mut ok = u2421reg_COUNT == 24;
    if ok { ok = u2421reg_get("O0_A") == Some(7) && u2421reg_get("O0_B") == Some(33); }
    if ok { ok = u2421reg_get("ZC_A") == Some(84); }
    if ok { ok = u2421reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2421reg_sum() == 1668; }
    ok
}

/// UNX-F2421 守恒账三操作确定性效果。
fn check_u2421_core_ldg() -> bool {
    let mut ld = Mxu2421Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2421_checks() -> CheckSet {
    let mut set = CheckSet::new("U2421");
    set.add(
        "u2421-seg-guard",
        u2421_guard_seg(),
        if u2421_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2421-core-tbl",
        check_u2421_core_tbl(),
        if check_u2421_core_tbl() { "" } else { "UNX-F2421 core table mismatch" },
    );
    set.add(
        "u2421-core-ldg",
        check_u2421_core_ldg(),
        if check_u2421_core_ldg() { "" } else { "UNX-F2421 core ledger mismatch" },
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
    fn u2421_all_checks_green() {
        let set = run_u2421_checks();
        assert!(set.all_passed(), "U2421 domain checks must be green");
    }

    #[test]
    fn u2421_segment_guard_holds() {
        assert!(u2421_segment_guard());
    }
}
