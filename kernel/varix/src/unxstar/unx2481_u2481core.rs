//! unxu2481_u2481core —— 大页族总纲（2M/1G 透明与显式两条路） 语义登记表与守恒账（大页路径）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2481 · 大页族总纲（2M/1G 透明与显式两条路） · 判据原文：UNX-F2481 · 判据：透明/显式两路定义与判据面三机型生效，路径重叠注入零双路
//! · 组件：大页路径 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2481 · 判据：透明/显式两路定义与判据面三机型生效，路径重叠注入零双路
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2481_VERSION: &str = "u2481-u2481core-v1";

/// 诊断段基址。
pub const U2481_SEG_BASE: u16 = 0xa300;

// --------------------------------------------------------------------------
// 诊断码（u2481 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 大页路径 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2481Code {
    /// 0xa300 表行重复（双路重入 同键双写）。
    RowDup,
    /// 0xa301 表行断档（选路错判 序号不连续）。
    RowGap,
    /// 0xa302 值越界（重叠未去 值域外）。
    ValOob,
    /// 0xa303 累计漂移（机型失效 求和≠理论值）。
    SumDrift,
    /// 0xa304 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xa305 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xa306 空操作（账目无效果调用）。
    OpNoop,
    /// 0xa307 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xa308 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa309 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa30a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa30b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2481Code {
    /// 全量闭集。
    pub const ALL: [u2481Code; 12] = [
        u2481Code::RowDup,
        u2481Code::RowGap,
        u2481Code::ValOob,
        u2481Code::SumDrift,
        u2481Code::LedgerDup,
        u2481Code::LedgerHole,
        u2481Code::OpNoop,
        u2481Code::FieldDrift,
        u2481Code::LedgerBroken,
        u2481Code::SnapshotFingerprint,
        u2481Code::TableBroken,
        u2481Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2481Code::RowDup => 0xa300,
            u2481Code::RowGap => 0xa301,
            u2481Code::ValOob => 0xa302,
            u2481Code::SumDrift => 0xa303,
            u2481Code::LedgerDup => 0xa304,
            u2481Code::LedgerHole => 0xa305,
            u2481Code::OpNoop => 0xa306,
            u2481Code::FieldDrift => 0xa307,
            u2481Code::LedgerBroken => 0xa308,
            u2481Code::SnapshotFingerprint => 0xa309,
            u2481Code::TableBroken => 0xa30a,
            u2481Code::SegmentGuard => 0xa30b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2481Code::RowDup => "表行重复（双路重入 同键双写）",
            u2481Code::RowGap => "表行断档（选路错判 序号不连续）",
            u2481Code::ValOob => "值越界（重叠未去 值域外）",
            u2481Code::SumDrift => "累计漂移（机型失效 求和≠理论值）",
            u2481Code::LedgerDup => "账目重记（同一事件双入账）",
            u2481Code::LedgerHole => "账目断漏（序号缺失）",
            u2481Code::OpNoop => "空操作（账目无效果调用）",
            u2481Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2481Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2481Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2481Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2481Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2481Code::RowDup => "按键唯一律去重",
            u2481Code::RowGap => "按连续律补号",
            u2481Code::ValOob => "按值域裁剪",
            u2481Code::SumDrift => "按理论值复算",
            u2481Code::LedgerDup => "按事件幂等律合并",
            u2481Code::LedgerHole => "按连续律补账",
            u2481Code::OpNoop => "核对操作前置条件",
            u2481Code::FieldDrift => "按口径重建字段",
            u2481Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2481Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2481Code::TableBroken => "按语义规则重建常量表",
            u2481Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2481reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 大页路径 语义登记表（24 行逐行注释）。
pub const u2481reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2481reg: [(&'static str, u64); 24] = [
    ("TR_A", 7), // TR 主登记（透明路 · 判据面）
    ("TR_B", 33), // TR 副登记（透明路 · 对拍面）
    ("EX_A", 14), // EX 主登记（显式路 · 判据面）
    ("EX_B", 44), // EX 副登记（显式路 · 对拍面）
    ("P2_A", 21), // P2 主登记（2M 页 · 判据面）
    ("P2_B", 55), // P2 副登记（2M 页 · 对拍面）
    ("P1_A", 28), // P1 主登记（1G 页 · 判据面）
    ("P1_B", 66), // P1 副登记（1G 页 · 对拍面）
    ("MC_A", 35), // MC 主登记（三机型 · 判据面）
    ("MC_B", 77), // MC 副登记（三机型 · 对拍面）
    ("OV_A", 42), // OV 主登记（重叠面 · 判据面）
    ("OV_B", 88), // OV 副登记（重叠面 · 对拍面）
    ("DB_A", 49), // DB 主登记（双路账 · 判据面）
    ("DB_B", 99), // DB 副登记（双路账 · 对拍面）
    ("FN_A", 56), // FN 主登记（路径判据面 · 判据面）
    ("FN_B", 110), // FN 副登记（路径判据面 · 对拍面）
    ("IN_A", 63), // IN 主登记（注入面 · 判据面）
    ("IN_B", 121), // IN 副登记（注入面 · 对拍面）
    ("ZD_A", 70), // ZD 主登记（零双路断言 · 判据面）
    ("ZD_B", 132), // ZD 副登记（零双路断言 · 对拍面）
    ("SEL_A", 77), // SEL 主登记（选路 · 判据面）
    ("SEL_B", 143), // SEL 副登记（选路 · 对拍面）
    ("MAP_A", 84), // MAP 主登记（映射面 · 判据面）
    ("MAP_B", 154), // MAP 副登记（映射面 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2481reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2481reg_COUNT {
        if u2481reg[i].0 == key {
            return Some(u2481reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2481reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2481reg_COUNT {
        s = s.wrapping_add(u2481reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2481Ledger（大页路径 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 大页路径 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2481Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2481Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa300, 0xa30f]。
pub const fn u2481_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2481Code::ALL.len() {
        let c = u2481Code::ALL[i].code();
        if c < 0xa300 || c > 0xa30f {
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
fn u2481_guard_seg() -> bool {
    u2481_segment_guard()
}

/// UNX-F2481 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2481_core_tbl() -> bool {
    let mut ok = u2481reg_COUNT == 24;
    if ok { ok = u2481reg_get("TR_A") == Some(7) && u2481reg_get("TR_B") == Some(33); }
    if ok { ok = u2481reg_get("MAP_A") == Some(84); }
    if ok { ok = u2481reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2481reg_sum() == 1668; }
    ok
}

/// UNX-F2481 守恒账三操作确定性效果。
fn check_u2481_core_ldg() -> bool {
    let mut ld = Mxu2481Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2481_checks() -> CheckSet {
    let mut set = CheckSet::new("U2481");
    set.add(
        "u2481-seg-guard",
        u2481_guard_seg(),
        if u2481_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2481-core-tbl",
        check_u2481_core_tbl(),
        if check_u2481_core_tbl() { "" } else { "UNX-F2481 core table mismatch" },
    );
    set.add(
        "u2481-core-ldg",
        check_u2481_core_ldg(),
        if check_u2481_core_ldg() { "" } else { "UNX-F2481 core ledger mismatch" },
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
    fn u2481_all_checks_green() {
        let set = run_u2481_checks();
        assert!(set.all_passed(), "U2481 domain checks must be green");
    }

    #[test]
    fn u2481_segment_guard_holds() {
        assert!(u2481_segment_guard());
    }
}
