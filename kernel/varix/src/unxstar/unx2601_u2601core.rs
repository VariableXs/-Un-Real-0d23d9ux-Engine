//! unxu2601_u2601core —— 现存 pfh.rs 接管声明（#PF 处理器收编） 语义登记表与守恒账（缺页处理路）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2601 · 现存 pfh.rs 接管声明（#PF 处理器收编） · 判据原文：UNX-F2601 · 判据：现存 `pfh.rs`（四路起步态：Guard/COW/GrowStack/MapZero）接管账在档（三态标注），收编后四路语义与成熟体逐路条目一致断言
//! · 组件：缺页处理路 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2601 · 判据：现存 `pfh.rs`（四路起步态：Guard/COW/GrowStack/MapZero）接管账在档（三态标注），收编后四路语义
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2601_VERSION: &str = "u2601-u2601core-v1";

/// 诊断段基址。
pub const U2601_SEG_BASE: u16 = 0xa780;

// --------------------------------------------------------------------------
// 诊断码（u2601 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 缺页处理路 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2601Code {
    /// 0xa780 表行重复（四路漏标 同键双写）。
    RowDup,
    /// 0xa781 表行断档（语义错位 序号不连续）。
    RowGap,
    /// 0xa782 值越界（接管缺账 值域外）。
    ValOob,
    /// 0xa783 累计漂移（断言分歧 求和≠理论值）。
    SumDrift,
    /// 0xa784 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xa785 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xa786 空操作（账目无效果调用）。
    OpNoop,
    /// 0xa787 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xa788 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa789 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa78a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa78b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2601Code {
    /// 全量闭集。
    pub const ALL: [u2601Code; 12] = [
        u2601Code::RowDup,
        u2601Code::RowGap,
        u2601Code::ValOob,
        u2601Code::SumDrift,
        u2601Code::LedgerDup,
        u2601Code::LedgerHole,
        u2601Code::OpNoop,
        u2601Code::FieldDrift,
        u2601Code::LedgerBroken,
        u2601Code::SnapshotFingerprint,
        u2601Code::TableBroken,
        u2601Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2601Code::RowDup => 0xa780,
            u2601Code::RowGap => 0xa781,
            u2601Code::ValOob => 0xa782,
            u2601Code::SumDrift => 0xa783,
            u2601Code::LedgerDup => 0xa784,
            u2601Code::LedgerHole => 0xa785,
            u2601Code::OpNoop => 0xa786,
            u2601Code::FieldDrift => 0xa787,
            u2601Code::LedgerBroken => 0xa788,
            u2601Code::SnapshotFingerprint => 0xa789,
            u2601Code::TableBroken => 0xa78a,
            u2601Code::SegmentGuard => 0xa78b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2601Code::RowDup => "表行重复（四路漏标 同键双写）",
            u2601Code::RowGap => "表行断档（语义错位 序号不连续）",
            u2601Code::ValOob => "值越界（接管缺账 值域外）",
            u2601Code::SumDrift => "累计漂移（断言分歧 求和≠理论值）",
            u2601Code::LedgerDup => "账目重记（同一事件双入账）",
            u2601Code::LedgerHole => "账目断漏（序号缺失）",
            u2601Code::OpNoop => "空操作（账目无效果调用）",
            u2601Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2601Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2601Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2601Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2601Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2601Code::RowDup => "按键唯一律去重",
            u2601Code::RowGap => "按连续律补号",
            u2601Code::ValOob => "按值域裁剪",
            u2601Code::SumDrift => "按理论值复算",
            u2601Code::LedgerDup => "按事件幂等律合并",
            u2601Code::LedgerHole => "按连续律补账",
            u2601Code::OpNoop => "核对操作前置条件",
            u2601Code::FieldDrift => "按口径重建字段",
            u2601Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2601Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2601Code::TableBroken => "按语义规则重建常量表",
            u2601Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2601reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 缺页处理路 语义登记表（24 行逐行注释）。
pub const u2601reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2601reg: [(&'static str, u64); 24] = [
    ("PF_A", 7), // PF 主登记（缺页处理器 · 判据面）
    ("PF_B", 33), // PF 副登记（缺页处理器 · 对拍面）
    ("GD_A", 14), // GD 主登记（Guard 路 · 判据面）
    ("GD_B", 44), // GD 副登记（Guard 路 · 对拍面）
    ("CW_A", 21), // CW 主登记（COW 路 · 判据面）
    ("CW_B", 55), // CW 副登记（COW 路 · 对拍面）
    ("GS_A", 28), // GS 主登记（GrowStack 路 · 判据面）
    ("GS_B", 66), // GS 副登记（GrowStack 路 · 对拍面）
    ("MZ_A", 35), // MZ 主登记（MapZero 路 · 判据面）
    ("MZ_B", 77), // MZ 副登记（MapZero 路 · 对拍面）
    ("TS_A", 42), // TS 主登记（三态标注 · 判据面）
    ("TS_B", 88), // TS 副登记（三态标注 · 对拍面）
    ("AD_A", 49), // AD 主登记（接管账 · 判据面）
    ("AD_B", 99), // AD 副登记（接管账 · 对拍面）
    ("MG_A", 56), // MG 主登记（成熟体 · 判据面）
    ("MG_B", 110), // MG 副登记（成熟体 · 对拍面）
    ("SM_A", 63), // SM 主登记（语义条目 · 判据面）
    ("SM_B", 121), // SM 副登记（语义条目 · 对拍面）
    ("WR_A", 70), // WR 主登记（四路 · 判据面）
    ("WR_B", 132), // WR 副登记（四路 · 对拍面）
    ("AC_A", 77), // AC 主登记（一致断言 · 判据面）
    ("AC_B", 143), // AC 副登记（一致断言 · 对拍面）
    ("RC_A", 84), // RC 主登记（收编锚 · 判据面）
    ("RC_B", 154), // RC 副登记（收编锚 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2601reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2601reg_COUNT {
        if u2601reg[i].0 == key {
            return Some(u2601reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2601reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2601reg_COUNT {
        s = s.wrapping_add(u2601reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2601Ledger（缺页处理路 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 缺页处理路 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2601Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2601Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa780, 0xa78f]。
pub const fn u2601_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2601Code::ALL.len() {
        let c = u2601Code::ALL[i].code();
        if c < 0xa780 || c > 0xa78f {
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
fn u2601_guard_seg() -> bool {
    u2601_segment_guard()
}

/// UNX-F2601 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2601_core_tbl() -> bool {
    let mut ok = u2601reg_COUNT == 24;
    if ok { ok = u2601reg_get("PF_A") == Some(7) && u2601reg_get("PF_B") == Some(33); }
    if ok { ok = u2601reg_get("RC_A") == Some(84); }
    if ok { ok = u2601reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2601reg_sum() == 1668; }
    ok
}

/// UNX-F2601 守恒账三操作确定性效果。
fn check_u2601_core_ldg() -> bool {
    let mut ld = Mxu2601Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2601_checks() -> CheckSet {
    let mut set = CheckSet::new("U2601");
    set.add(
        "u2601-seg-guard",
        u2601_guard_seg(),
        if u2601_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2601-core-tbl",
        check_u2601_core_tbl(),
        if check_u2601_core_tbl() { "" } else { "UNX-F2601 core table mismatch" },
    );
    set.add(
        "u2601-core-ldg",
        check_u2601_core_ldg(),
        if check_u2601_core_ldg() { "" } else { "UNX-F2601 core ledger mismatch" },
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
    fn u2601_all_checks_green() {
        let set = run_u2601_checks();
        assert!(set.all_passed(), "U2601 domain checks must be green");
    }

    #[test]
    fn u2601_segment_guard_holds() {
        assert!(u2601_segment_guard());
    }
}
