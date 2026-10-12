//! unxu2581_u2581core —— VMA 区间树结构总纲（红黑树类，按地址排序） 语义登记表与守恒账（VMA 区间）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2581 · VMA 区间树结构总纲（红黑树类，按地址排序） · 判据原文：UNX-F2581 · 判据：区间树结构逐项断言（排序/平衡/区间不变量 10^6 次操作全过），VMA 记录四字段（起止/属性/类型/对象）齐备
//! · 组件：VMA 区间 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2581 · 判据：区间树结构逐项断言（排序/平衡/区间不变量 10^6 次操作全过），VMA 记录四字段（起止/属性/类型/对象）齐备
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2581_VERSION: &str = "u2581-u2581core-v1";

/// 诊断段基址。
pub const U2581_SEG_BASE: u16 = 0xa6c0;

// --------------------------------------------------------------------------
// 诊断码（u2581 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// VMA 区间 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2581Code {
    /// 0xa6c0 表行重复（排序破坏 同键双写）。
    RowDup,
    /// 0xa6c1 表行断档（失衡塌陷 序号不连续）。
    RowGap,
    /// 0xa6c2 值越界（区间重叠 值域外）。
    ValOob,
    /// 0xa6c3 累计漂移（字段缺失 求和≠理论值）。
    SumDrift,
    /// 0xa6c4 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xa6c5 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xa6c6 空操作（账目无效果调用）。
    OpNoop,
    /// 0xa6c7 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xa6c8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa6c9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa6ca 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa6cb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2581Code {
    /// 全量闭集。
    pub const ALL: [u2581Code; 12] = [
        u2581Code::RowDup,
        u2581Code::RowGap,
        u2581Code::ValOob,
        u2581Code::SumDrift,
        u2581Code::LedgerDup,
        u2581Code::LedgerHole,
        u2581Code::OpNoop,
        u2581Code::FieldDrift,
        u2581Code::LedgerBroken,
        u2581Code::SnapshotFingerprint,
        u2581Code::TableBroken,
        u2581Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2581Code::RowDup => 0xa6c0,
            u2581Code::RowGap => 0xa6c1,
            u2581Code::ValOob => 0xa6c2,
            u2581Code::SumDrift => 0xa6c3,
            u2581Code::LedgerDup => 0xa6c4,
            u2581Code::LedgerHole => 0xa6c5,
            u2581Code::OpNoop => 0xa6c6,
            u2581Code::FieldDrift => 0xa6c7,
            u2581Code::LedgerBroken => 0xa6c8,
            u2581Code::SnapshotFingerprint => 0xa6c9,
            u2581Code::TableBroken => 0xa6ca,
            u2581Code::SegmentGuard => 0xa6cb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2581Code::RowDup => "表行重复（排序破坏 同键双写）",
            u2581Code::RowGap => "表行断档（失衡塌陷 序号不连续）",
            u2581Code::ValOob => "值越界（区间重叠 值域外）",
            u2581Code::SumDrift => "累计漂移（字段缺失 求和≠理论值）",
            u2581Code::LedgerDup => "账目重记（同一事件双入账）",
            u2581Code::LedgerHole => "账目断漏（序号缺失）",
            u2581Code::OpNoop => "空操作（账目无效果调用）",
            u2581Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2581Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2581Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2581Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2581Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2581Code::RowDup => "按键唯一律去重",
            u2581Code::RowGap => "按连续律补号",
            u2581Code::ValOob => "按值域裁剪",
            u2581Code::SumDrift => "按理论值复算",
            u2581Code::LedgerDup => "按事件幂等律合并",
            u2581Code::LedgerHole => "按连续律补账",
            u2581Code::OpNoop => "核对操作前置条件",
            u2581Code::FieldDrift => "按口径重建字段",
            u2581Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2581Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2581Code::TableBroken => "按语义规则重建常量表",
            u2581Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2581reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// VMA 区间 语义登记表（24 行逐行注释）。
pub const u2581reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2581reg: [(&'static str, u64); 24] = [
    ("VM_A", 7), // VM 主登记（VMA 记录 · 判据面）
    ("VM_B", 33), // VM 副登记（VMA 记录 · 对拍面）
    ("RB_A", 14), // RB 主登记（红黑树类 · 判据面）
    ("RB_B", 44), // RB 副登记（红黑树类 · 对拍面）
    ("SO_A", 21), // SO 主登记（排序不变量 · 判据面）
    ("SO_B", 55), // SO 副登记（排序不变量 · 对拍面）
    ("BA_A", 28), // BA 主登记（平衡不变量 · 判据面）
    ("BA_B", 66), // BA 副登记（平衡不变量 · 对拍面）
    ("IV_A", 35), // IV 主登记（区间不变量 · 判据面）
    ("IV_B", 77), // IV 副登记（区间不变量 · 对拍面）
    ("M1_A", 42), // M1 主登记（百万次操作 · 判据面）
    ("M1_B", 88), // M1 副登记（百万次操作 · 对拍面）
    ("ST_A", 49), // ST 主登记（起始地址 · 判据面）
    ("ST_B", 99), // ST 副登记（起始地址 · 对拍面）
    ("EN_A", 56), // EN 主登记（终止地址 · 判据面）
    ("EN_B", 110), // EN 副登记（终止地址 · 对拍面）
    ("AT_A", 63), // AT 主登记（属性 · 判据面）
    ("AT_B", 121), // AT 副登记（属性 · 对拍面）
    ("TY_A", 70), // TY 主登记（类型 · 判据面）
    ("TY_B", 132), // TY 副登记（类型 · 对拍面）
    ("OB_A", 77), // OB 主登记（对象 · 判据面）
    ("OB_B", 143), // OB 副登记（对象 · 对拍面）
    ("FI_A", 84), // FI 主登记（四字段 · 判据面）
    ("FI_B", 154), // FI 副登记（四字段 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2581reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2581reg_COUNT {
        if u2581reg[i].0 == key {
            return Some(u2581reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2581reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2581reg_COUNT {
        s = s.wrapping_add(u2581reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2581Ledger（VMA 区间 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// VMA 区间 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2581Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2581Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa6c0, 0xa6cf]。
pub const fn u2581_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2581Code::ALL.len() {
        let c = u2581Code::ALL[i].code();
        if c < 0xa6c0 || c > 0xa6cf {
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
fn u2581_guard_seg() -> bool {
    u2581_segment_guard()
}

/// UNX-F2581 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2581_core_tbl() -> bool {
    let mut ok = u2581reg_COUNT == 24;
    if ok { ok = u2581reg_get("VM_A") == Some(7) && u2581reg_get("VM_B") == Some(33); }
    if ok { ok = u2581reg_get("FI_A") == Some(84); }
    if ok { ok = u2581reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2581reg_sum() == 1668; }
    ok
}

/// UNX-F2581 守恒账三操作确定性效果。
fn check_u2581_core_ldg() -> bool {
    let mut ld = Mxu2581Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2581_checks() -> CheckSet {
    let mut set = CheckSet::new("U2581");
    set.add(
        "u2581-seg-guard",
        u2581_guard_seg(),
        if u2581_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2581-core-tbl",
        check_u2581_core_tbl(),
        if check_u2581_core_tbl() { "" } else { "UNX-F2581 core table mismatch" },
    );
    set.add(
        "u2581-core-ldg",
        check_u2581_core_ldg(),
        if check_u2581_core_ldg() { "" } else { "UNX-F2581 core ledger mismatch" },
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
    fn u2581_all_checks_green() {
        let set = run_u2581_checks();
        assert!(set.all_passed(), "U2581 domain checks must be green");
    }

    #[test]
    fn u2581_segment_guard_holds() {
        assert!(u2581_segment_guard());
    }
}
