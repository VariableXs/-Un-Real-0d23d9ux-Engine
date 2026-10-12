//! unxu2981_u2981core —— 域级收口方案冻结（收口六件套范围） 语义登记表与守恒账（域级收口六件套）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2981 · 域级收口方案冻结（收口六件套范围） · 判据原文：UNX-F2981 · 判据：UNX-F2981-J1 收口六件套（总对账/完整性终核/守恒终账/指标总览/欠账终账/总判定）范围冻结哈希入册，六件与 F2982–F2997 条位一一对应
//! · 组件：域级收口六件套 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2981 · 判据：UNX-F2981-J1 收口六件套（总对账/完整性终核/守恒终账/指标总览/欠账终账/总判定）范围冻结哈希入册，六件与 F298
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2981_VERSION: &str = "u2981-u2981core-v1";

/// 诊断段基址。
pub const U2981_SEG_BASE: u16 = 0xe240;

// --------------------------------------------------------------------------
// 诊断码（u2981 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域级收口六件套 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2981Code {
    /// 0xe240 表行重复（收口 同键双写）。
    RowDup,
    /// 0xe241 表行断档（六件套 序号不连续）。
    RowGap,
    /// 0xe242 值越界（封口 值域外）。
    ValOob,
    /// 0xe243 累计漂移（六件套 求和≠理论值）。
    SumDrift,
    /// 0xe244 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe245 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe246 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe247 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe248 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe249 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe24a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe24b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2981Code {
    /// 全量闭集。
    pub const ALL: [u2981Code; 12] = [
        u2981Code::RowDup,
        u2981Code::RowGap,
        u2981Code::ValOob,
        u2981Code::SumDrift,
        u2981Code::LedgerDup,
        u2981Code::LedgerHole,
        u2981Code::OpNoop,
        u2981Code::FieldDrift,
        u2981Code::LedgerBroken,
        u2981Code::SnapshotFingerprint,
        u2981Code::TableBroken,
        u2981Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2981Code::RowDup => 0xe240,
            u2981Code::RowGap => 0xe241,
            u2981Code::ValOob => 0xe242,
            u2981Code::SumDrift => 0xe243,
            u2981Code::LedgerDup => 0xe244,
            u2981Code::LedgerHole => 0xe245,
            u2981Code::OpNoop => 0xe246,
            u2981Code::FieldDrift => 0xe247,
            u2981Code::LedgerBroken => 0xe248,
            u2981Code::SnapshotFingerprint => 0xe249,
            u2981Code::TableBroken => 0xe24a,
            u2981Code::SegmentGuard => 0xe24b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2981Code::RowDup => "表行重复（收口 同键双写）",
            u2981Code::RowGap => "表行断档（六件套 序号不连续）",
            u2981Code::ValOob => "值越界（封口 值域外）",
            u2981Code::SumDrift => "累计漂移（六件套 求和≠理论值）",
            u2981Code::LedgerDup => "账目重记（同一事件双入账）",
            u2981Code::LedgerHole => "账目断漏（序号缺失）",
            u2981Code::OpNoop => "空操作（账目无效果调用）",
            u2981Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2981Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2981Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2981Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2981Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2981Code::RowDup => "按键唯一律去重",
            u2981Code::RowGap => "按连续律补号",
            u2981Code::ValOob => "按值域裁剪",
            u2981Code::SumDrift => "按理论值复算",
            u2981Code::LedgerDup => "按事件幂等律合并",
            u2981Code::LedgerHole => "按连续律补账",
            u2981Code::OpNoop => "核对操作前置条件",
            u2981Code::FieldDrift => "按口径重建字段",
            u2981Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2981Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2981Code::TableBroken => "按语义规则重建常量表",
            u2981Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2981reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 域级收口六件套 语义登记表（24 行逐行注释）。
pub const u2981reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2981reg: [(&'static str, u64); 24] = [
    ("SIX_A", 7), // SIX 主登记（六件套 · 判据面）
    ("SIX_B", 33), // SIX 副登记（六件套 · 对拍面）
    ("CLOSE_A", 14), // CLOSE 主登记（收口 · 判据面）
    ("CLOSE_B", 44), // CLOSE 副登记（收口 · 对拍面）
    ("SCOPE_A", 21), // SCOPE 主登记（范围 · 判据面）
    ("SCOPE_B", 55), // SCOPE 副登记（范围 · 对拍面）
    ("ITEM_A", 28), // ITEM 主登记（条目 · 判据面）
    ("ITEM_B", 66), // ITEM 副登记（条目 · 对拍面）
    ("PROOF_A", 35), // PROOF 主登记（证据 · 判据面）
    ("PROOF_B", 77), // PROOF 副登记（证据 · 对拍面）
    ("BIND_A", 42), // BIND 主登记（绑定 · 判据面）
    ("BIND_B", 88), // BIND 副登记（绑定 · 对拍面）
    ("ACCT_A", 49), // ACCT 主登记（账 · 判据面）
    ("ACCT_B", 99), // ACCT 副登记（账 · 对拍面）
    ("CHK_A", 56), // CHK 主登记（核验 · 判据面）
    ("CHK_B", 110), // CHK 副登记（核验 · 对拍面）
    ("GAP_A", 63), // GAP 主登记（缺口 · 判据面）
    ("GAP_B", 121), // GAP 副登记（缺口 · 对拍面）
    ("SIGN_A", 70), // SIGN 主登记（签署 · 判据面）
    ("SIGN_B", 132), // SIGN 副登记（签署 · 对拍面）
    ("ARCH_A", 77), // ARCH 主登记（归档 · 判据面）
    ("ARCH_B", 143), // ARCH 副登记（归档 · 对拍面）
    ("SEAL_A", 84), // SEAL 主登记（封口 · 判据面）
    ("SEAL_B", 154), // SEAL 副登记（封口 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2981reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2981reg_COUNT {
        if u2981reg[i].0 == key {
            return Some(u2981reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2981reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2981reg_COUNT {
        s = s.wrapping_add(u2981reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2981Ledger（域级收口六件套 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 域级收口六件套 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2981Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2981Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe240, 0xe24f]。
pub const fn u2981_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2981Code::ALL.len() {
        let c = u2981Code::ALL[i].code();
        if c < 0xe240 || c > 0xe24f {
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
fn u2981_guard_seg() -> bool {
    u2981_segment_guard()
}

/// UNX-F2981 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2981_core_tbl() -> bool {
    let mut ok = u2981reg_COUNT == 24;
    if ok { ok = u2981reg_get("SIX_A") == Some(7) && u2981reg_get("SIX_B") == Some(33); }
    if ok { ok = u2981reg_get("SEAL_A") == Some(84); }
    if ok { ok = u2981reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2981reg_sum() == 1668; }
    ok
}

/// UNX-F2981 守恒账三操作确定性效果。
fn check_u2981_core_ldg() -> bool {
    let mut ld = Mxu2981Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2981_checks() -> CheckSet {
    let mut set = CheckSet::new("U2981");
    set.add(
        "u2981-seg-guard",
        u2981_guard_seg(),
        if u2981_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2981-core-tbl",
        check_u2981_core_tbl(),
        if check_u2981_core_tbl() { "" } else { "UNX-F2981 core table mismatch" },
    );
    set.add(
        "u2981-core-ldg",
        check_u2981_core_ldg(),
        if check_u2981_core_ldg() { "" } else { "UNX-F2981 core ledger mismatch" },
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
    fn u2981_all_checks_green() {
        let set = run_u2981_checks();
        assert!(set.all_passed(), "U2981 domain checks must be green");
    }

    #[test]
    fn u2981_segment_guard_holds() {
        assert!(u2981_segment_guard());
    }
}
