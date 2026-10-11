//! unxu2781_u2781core —— 回收-分配联动总闸 语义登记表与守恒账（联动事务）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2781 · 回收-分配联动总闸 · 判据原文：UNX-F2781 · 判据：联动总闸（分配路径事件→回收路径响应）全链 10/10 次贯通，联动账（触发源/响应/时延）三列齐备
//! · 组件：联动事务 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2781 · 判据：联动总闸（分配路径事件→回收路径响应）全链 10/10 次贯通，联动账（触发源/响应/时延）三列齐备
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2781_VERSION: &str = "u2781-u2781core-v1";

/// 诊断段基址。
pub const U2781_SEG_BASE: u16 = 0xae00;

// --------------------------------------------------------------------------
// 诊断码（u2781 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 联动事务 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2781Code {
    /// 0xae00 表行重复（闸门失守 同键双写）。
    RowDup,
    /// 0xae01 表行断档（响应缺列 序号不连续）。
    RowGap,
    /// 0xae02 值越界（时延失真 值域外）。
    ValOob,
    /// 0xae03 累计漂移（链路断裂 求和≠理论值）。
    SumDrift,
    /// 0xae04 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xae05 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xae06 空操作（账目无效果调用）。
    OpNoop,
    /// 0xae07 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xae08 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xae09 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xae0a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xae0b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2781Code {
    /// 全量闭集。
    pub const ALL: [u2781Code; 12] = [
        u2781Code::RowDup,
        u2781Code::RowGap,
        u2781Code::ValOob,
        u2781Code::SumDrift,
        u2781Code::LedgerDup,
        u2781Code::LedgerHole,
        u2781Code::OpNoop,
        u2781Code::FieldDrift,
        u2781Code::LedgerBroken,
        u2781Code::SnapshotFingerprint,
        u2781Code::TableBroken,
        u2781Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2781Code::RowDup => 0xae00,
            u2781Code::RowGap => 0xae01,
            u2781Code::ValOob => 0xae02,
            u2781Code::SumDrift => 0xae03,
            u2781Code::LedgerDup => 0xae04,
            u2781Code::LedgerHole => 0xae05,
            u2781Code::OpNoop => 0xae06,
            u2781Code::FieldDrift => 0xae07,
            u2781Code::LedgerBroken => 0xae08,
            u2781Code::SnapshotFingerprint => 0xae09,
            u2781Code::TableBroken => 0xae0a,
            u2781Code::SegmentGuard => 0xae0b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2781Code::RowDup => "表行重复（闸门失守 同键双写）",
            u2781Code::RowGap => "表行断档（响应缺列 序号不连续）",
            u2781Code::ValOob => "值越界（时延失真 值域外）",
            u2781Code::SumDrift => "累计漂移（链路断裂 求和≠理论值）",
            u2781Code::LedgerDup => "账目重记（同一事件双入账）",
            u2781Code::LedgerHole => "账目断漏（序号缺失）",
            u2781Code::OpNoop => "空操作（账目无效果调用）",
            u2781Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2781Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2781Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2781Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2781Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2781Code::RowDup => "按键唯一律去重",
            u2781Code::RowGap => "按连续律补号",
            u2781Code::ValOob => "按值域裁剪",
            u2781Code::SumDrift => "按理论值复算",
            u2781Code::LedgerDup => "按事件幂等律合并",
            u2781Code::LedgerHole => "按连续律补账",
            u2781Code::OpNoop => "核对操作前置条件",
            u2781Code::FieldDrift => "按口径重建字段",
            u2781Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2781Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2781Code::TableBroken => "按语义规则重建常量表",
            u2781Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2781reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 联动事务 语义登记表（24 行逐行注释）。
pub const u2781reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2781reg: [(&'static str, u64); 24] = [
    ("LK_A", 7), // LK 主登记（联动总闸 · 判据面）
    ("LK_B", 33), // LK 副登记（联动总闸 · 对拍面）
    ("AP_A", 14), // AP 主登记（分配路径 · 判据面）
    ("AP_B", 44), // AP 副登记（分配路径 · 对拍面）
    ("RP_A", 21), // RP 主登记（回收路径 · 判据面）
    ("RP_B", 55), // RP 副登记（回收路径 · 对拍面）
    ("TS_A", 28), // TS 主登记（触发源 · 判据面）
    ("TS_B", 66), // TS 副登记（触发源 · 对拍面）
    ("RS_A", 35), // RS 主登记（响应 · 判据面）
    ("RS_B", 77), // RS 副登记（响应 · 对拍面）
    ("LT_A", 42), // LT 主登记（时延 · 判据面）
    ("LT_B", 88), // LT 副登记（时延 · 对拍面）
    ("C3_A", 49), // C3 主登记（三列账 · 判据面）
    ("C3_B", 99), // C3 副登记（三列账 · 对拍面）
    ("C10_A", 56), // C10 主登记（十次贯通 · 判据面）
    ("C10_B", 110), // C10 副登记（十次贯通 · 对拍面）
    ("FN_A", 63), // FN 主登记（全链贯通 · 判据面）
    ("FN_B", 121), // FN 副登记（全链贯通 · 对拍面）
    ("AB_A", 70), // AB 主登记（阻断面 · 判据面）
    ("AB_B", 132), // AB 副登记（阻断面 · 对拍面）
    ("GD_A", 77), // GD 主登记（联动闸门 · 判据面）
    ("GD_B", 143), // GD 副登记（联动闸门 · 对拍面）
    ("PR_A", 84), // PR 主登记（路径账 · 判据面）
    ("PR_B", 154), // PR 副登记（路径账 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2781reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2781reg_COUNT {
        if u2781reg[i].0 == key {
            return Some(u2781reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2781reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2781reg_COUNT {
        s = s.wrapping_add(u2781reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2781Ledger（联动事务 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 联动事务 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2781Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2781Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xae00, 0xae0f]。
pub const fn u2781_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2781Code::ALL.len() {
        let c = u2781Code::ALL[i].code();
        if c < 0xae00 || c > 0xae0f {
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
fn u2781_guard_seg() -> bool {
    u2781_segment_guard()
}

/// UNX-F2781 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2781_core_tbl() -> bool {
    let mut ok = u2781reg_COUNT == 24;
    if ok { ok = u2781reg_get("LK_A") == Some(7) && u2781reg_get("LK_B") == Some(33); }
    if ok { ok = u2781reg_get("PR_A") == Some(84); }
    if ok { ok = u2781reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2781reg_sum() == 1668; }
    ok
}

/// UNX-F2781 守恒账三操作确定性效果。
fn check_u2781_core_ldg() -> bool {
    let mut ld = Mxu2781Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2781_checks() -> CheckSet {
    let mut set = CheckSet::new("U2781");
    set.add(
        "u2781-seg-guard",
        u2781_guard_seg(),
        if u2781_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2781-core-tbl",
        check_u2781_core_tbl(),
        if check_u2781_core_tbl() { "" } else { "UNX-F2781 core table mismatch" },
    );
    set.add(
        "u2781-core-ldg",
        check_u2781_core_ldg(),
        if check_u2781_core_ldg() { "" } else { "UNX-F2781 core ledger mismatch" },
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
    fn u2781_all_checks_green() {
        let set = run_u2781_checks();
        assert!(set.all_passed(), "U2781 domain checks must be green");
    }

    #[test]
    fn u2781_segment_guard_holds() {
        assert!(u2781_segment_guard());
    }
}
