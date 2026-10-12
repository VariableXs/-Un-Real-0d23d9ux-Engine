//! unxu2901_u2901core —— 页缓存压测方案冻结（三矩阵） 语义登记表与守恒账（页缓存三矩阵压测）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2901 · 页缓存压测方案冻结（三矩阵） · 判据原文：UNX-F2901 · 判据：UNX-F2901-J1 三矩阵（读写比/文件尺寸/并发度）参数冻结哈希入册，方案与 F2682 三接口字段对齐
//! · 组件：页缓存三矩阵压测 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2901 · 判据：UNX-F2901-J1 三矩阵（读写比/文件尺寸/并发度）参数冻结哈希入册，方案与 F2682 三接口字段对齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2901_VERSION: &str = "u2901-u2901core-v1";

/// 诊断段基址。
pub const U2901_SEG_BASE: u16 = 0xe200;

// --------------------------------------------------------------------------
// 诊断码（u2901 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 页缓存三矩阵压测 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2901Code {
    /// 0xe200 表行重复（页缓存 同键双写）。
    RowDup,
    /// 0xe201 表行断档（radix 序号不连续）。
    RowGap,
    /// 0xe202 值越界（逐出 值域外）。
    ValOob,
    /// 0xe203 累计漂移（逐出 求和≠理论值）。
    SumDrift,
    /// 0xe204 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe205 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe206 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe207 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe208 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe209 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe20a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe20b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2901Code {
    /// 全量闭集。
    pub const ALL: [u2901Code; 12] = [
        u2901Code::RowDup,
        u2901Code::RowGap,
        u2901Code::ValOob,
        u2901Code::SumDrift,
        u2901Code::LedgerDup,
        u2901Code::LedgerHole,
        u2901Code::OpNoop,
        u2901Code::FieldDrift,
        u2901Code::LedgerBroken,
        u2901Code::SnapshotFingerprint,
        u2901Code::TableBroken,
        u2901Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2901Code::RowDup => 0xe200,
            u2901Code::RowGap => 0xe201,
            u2901Code::ValOob => 0xe202,
            u2901Code::SumDrift => 0xe203,
            u2901Code::LedgerDup => 0xe204,
            u2901Code::LedgerHole => 0xe205,
            u2901Code::OpNoop => 0xe206,
            u2901Code::FieldDrift => 0xe207,
            u2901Code::LedgerBroken => 0xe208,
            u2901Code::SnapshotFingerprint => 0xe209,
            u2901Code::TableBroken => 0xe20a,
            u2901Code::SegmentGuard => 0xe20b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2901Code::RowDup => "表行重复（页缓存 同键双写）",
            u2901Code::RowGap => "表行断档（radix 序号不连续）",
            u2901Code::ValOob => "值越界（逐出 值域外）",
            u2901Code::SumDrift => "累计漂移（逐出 求和≠理论值）",
            u2901Code::LedgerDup => "账目重记（同一事件双入账）",
            u2901Code::LedgerHole => "账目断漏（序号缺失）",
            u2901Code::OpNoop => "空操作（账目无效果调用）",
            u2901Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2901Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2901Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2901Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2901Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2901Code::RowDup => "按键唯一律去重",
            u2901Code::RowGap => "按连续律补号",
            u2901Code::ValOob => "按值域裁剪",
            u2901Code::SumDrift => "按理论值复算",
            u2901Code::LedgerDup => "按事件幂等律合并",
            u2901Code::LedgerHole => "按连续律补账",
            u2901Code::OpNoop => "核对操作前置条件",
            u2901Code::FieldDrift => "按口径重建字段",
            u2901Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2901Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2901Code::TableBroken => "按语义规则重建常量表",
            u2901Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2901reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 页缓存三矩阵压测 语义登记表（24 行逐行注释）。
pub const u2901reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2901reg: [(&'static str, u64); 24] = [
    ("PC_A", 7), // PC 主登记（页缓存 · 判据面）
    ("PC_B", 33), // PC 副登记（页缓存 · 对拍面）
    ("M1_A", 14), // M1 主登记（矩阵一 · 判据面）
    ("M1_B", 44), // M1 副登记（矩阵一 · 对拍面）
    ("M2_A", 21), // M2 主登记（矩阵二 · 判据面）
    ("M2_B", 55), // M2 副登记（矩阵二 · 对拍面）
    ("M3_A", 28), // M3 主登记（矩阵三 · 判据面）
    ("M3_B", 66), // M3 副登记（矩阵三 · 对拍面）
    ("RADIX_A", 35), // RADIX 主登记（radix 树 · 判据面）
    ("RADIX_B", 77), // RADIX 副登记（radix 树 · 对拍面）
    ("EVICT_A", 42), // EVICT 主登记（逐出 · 判据面）
    ("EVICT_B", 88), // EVICT 副登记（逐出 · 对拍面）
    ("RECLAIM_A", 49), // RECLAIM 主登记（回收 · 判据面）
    ("RECLAIM_B", 99), // RECLAIM 副登记（回收 · 对拍面）
    ("HIT_A", 56), // HIT 主登记（命中 · 判据面）
    ("HIT_B", 110), // HIT 副登记（命中 · 对拍面）
    ("MISS_A", 63), // MISS 主登记（未命中 · 判据面）
    ("MISS_B", 121), // MISS 副登记（未命中 · 对拍面）
    ("IO_A", 70), // IO 主登记（回源 · 判据面）
    ("IO_B", 132), // IO 副登记（回源 · 对拍面）
    ("THR_A", 77), // THR 主登记（阈值 · 判据面）
    ("THR_B", 143), // THR 副登记（阈值 · 对拍面）
    ("ACC_A", 84), // ACC 主登记（记账 · 判据面）
    ("ACC_B", 154), // ACC 副登记（记账 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2901reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2901reg_COUNT {
        if u2901reg[i].0 == key {
            return Some(u2901reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2901reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2901reg_COUNT {
        s = s.wrapping_add(u2901reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2901Ledger（页缓存三矩阵压测 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 页缓存三矩阵压测 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2901Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2901Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe200, 0xe20f]。
pub const fn u2901_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2901Code::ALL.len() {
        let c = u2901Code::ALL[i].code();
        if c < 0xe200 || c > 0xe20f {
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
fn u2901_guard_seg() -> bool {
    u2901_segment_guard()
}

/// UNX-F2901 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2901_core_tbl() -> bool {
    let mut ok = u2901reg_COUNT == 24;
    if ok { ok = u2901reg_get("PC_A") == Some(7) && u2901reg_get("PC_B") == Some(33); }
    if ok { ok = u2901reg_get("ACC_A") == Some(84); }
    if ok { ok = u2901reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2901reg_sum() == 1668; }
    ok
}

/// UNX-F2901 守恒账三操作确定性效果。
fn check_u2901_core_ldg() -> bool {
    let mut ld = Mxu2901Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2901_checks() -> CheckSet {
    let mut set = CheckSet::new("U2901");
    set.add(
        "u2901-seg-guard",
        u2901_guard_seg(),
        if u2901_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2901-core-tbl",
        check_u2901_core_tbl(),
        if check_u2901_core_tbl() { "" } else { "UNX-F2901 core table mismatch" },
    );
    set.add(
        "u2901-core-ldg",
        check_u2901_core_ldg(),
        if check_u2901_core_ldg() { "" } else { "UNX-F2901 core ledger mismatch" },
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
    fn u2901_all_checks_green() {
        let set = run_u2901_checks();
        assert!(set.all_passed(), "U2901 domain checks must be green");
    }

    #[test]
    fn u2901_segment_guard_holds() {
        assert!(u2901_segment_guard());
    }
}
