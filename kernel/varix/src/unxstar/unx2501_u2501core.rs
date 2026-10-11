//! unxu2501_u2501core —— 大页映射前向接口冻结（→B10 交接冻结件） 语义登记表与守恒账（冻结接口）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2501 · 大页映射前向接口冻结（→B10 交接冻结件） · 判据原文：UNX-F2501 · 判据：大页映射前向接口冻结件登记（B10 消费联签），接口字段与池账口径一致
//! · 组件：冻结接口 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2501 · 判据：大页映射前向接口冻结件登记（B10 消费联签），接口字段与池账口径一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2501_VERSION: &str = "u2501-u2501core-v1";

/// 诊断段基址。
pub const U2501_SEG_BASE: u16 = 0xa3c0;

// --------------------------------------------------------------------------
// 诊断码（u2501 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 冻结接口 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2501Code {
    /// 0xa3c0 表行重复（字段缺项 同键双写）。
    RowDup,
    /// 0xa3c1 表行断档（口径分歧 序号不连续）。
    RowGap,
    /// 0xa3c2 值越界（联签缺失 值域外）。
    ValOob,
    /// 0xa3c3 累计漂移（冻结漂移 求和≠理论值）。
    SumDrift,
    /// 0xa3c4 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xa3c5 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xa3c6 空操作（账目无效果调用）。
    OpNoop,
    /// 0xa3c7 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xa3c8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa3c9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa3ca 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa3cb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2501Code {
    /// 全量闭集。
    pub const ALL: [u2501Code; 12] = [
        u2501Code::RowDup,
        u2501Code::RowGap,
        u2501Code::ValOob,
        u2501Code::SumDrift,
        u2501Code::LedgerDup,
        u2501Code::LedgerHole,
        u2501Code::OpNoop,
        u2501Code::FieldDrift,
        u2501Code::LedgerBroken,
        u2501Code::SnapshotFingerprint,
        u2501Code::TableBroken,
        u2501Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2501Code::RowDup => 0xa3c0,
            u2501Code::RowGap => 0xa3c1,
            u2501Code::ValOob => 0xa3c2,
            u2501Code::SumDrift => 0xa3c3,
            u2501Code::LedgerDup => 0xa3c4,
            u2501Code::LedgerHole => 0xa3c5,
            u2501Code::OpNoop => 0xa3c6,
            u2501Code::FieldDrift => 0xa3c7,
            u2501Code::LedgerBroken => 0xa3c8,
            u2501Code::SnapshotFingerprint => 0xa3c9,
            u2501Code::TableBroken => 0xa3ca,
            u2501Code::SegmentGuard => 0xa3cb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2501Code::RowDup => "表行重复（字段缺项 同键双写）",
            u2501Code::RowGap => "表行断档（口径分歧 序号不连续）",
            u2501Code::ValOob => "值越界（联签缺失 值域外）",
            u2501Code::SumDrift => "累计漂移（冻结漂移 求和≠理论值）",
            u2501Code::LedgerDup => "账目重记（同一事件双入账）",
            u2501Code::LedgerHole => "账目断漏（序号缺失）",
            u2501Code::OpNoop => "空操作（账目无效果调用）",
            u2501Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2501Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2501Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2501Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2501Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2501Code::RowDup => "按键唯一律去重",
            u2501Code::RowGap => "按连续律补号",
            u2501Code::ValOob => "按值域裁剪",
            u2501Code::SumDrift => "按理论值复算",
            u2501Code::LedgerDup => "按事件幂等律合并",
            u2501Code::LedgerHole => "按连续律补账",
            u2501Code::OpNoop => "核对操作前置条件",
            u2501Code::FieldDrift => "按口径重建字段",
            u2501Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2501Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2501Code::TableBroken => "按语义规则重建常量表",
            u2501Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2501reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 冻结接口 语义登记表（24 行逐行注释）。
pub const u2501reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2501reg: [(&'static str, u64); 24] = [
    ("IF_A", 7), // IF 主登记（前向接口 · 判据面）
    ("IF_B", 33), // IF 副登记（前向接口 · 对拍面）
    ("FR_A", 14), // FR 主登记（冻结件 · 判据面）
    ("FR_B", 44), // FR 副登记（冻结件 · 对拍面）
    ("B10_A", 21), // B10 主登记（B10 联签 · 判据面）
    ("B10_B", 55), // B10 副登记（B10 联签 · 对拍面）
    ("FL_A", 28), // FL 主登记（字段表 · 判据面）
    ("FL_B", 66), // FL 副登记（字段表 · 对拍面）
    ("PC_A", 35), // PC 主登记（池账口径 · 判据面）
    ("PC_B", 77), // PC 副登记（池账口径 · 对拍面）
    ("SG_A", 42), // SG 主登记（签名账 · 判据面）
    ("SG_B", 88), // SG 副登记（签名账 · 对拍面）
    ("CN_A", 49), // CN 主登记（消费方 · 判据面）
    ("CN_B", 99), // CN 副登记（消费方 · 对拍面）
    ("HD_A", 56), // HD 主登记（交接面 · 判据面）
    ("HD_B", 110), // HD 副登记（交接面 · 对拍面）
    ("VF_A", 63), // VF 主登记（字段核验 · 判据面）
    ("VF_B", 121), // VF 副登记（字段核验 · 对拍面）
    ("EQ_A", 70), // EQ 主登记（口径一致 · 判据面）
    ("EQ_B", 132), // EQ 副登记（口径一致 · 对拍面）
    ("LK_A", 77), // LK 主登记（联签账 · 判据面）
    ("LK_B", 143), // LK 副登记（联签账 · 对拍面）
    ("FZ_A", 84), // FZ 主登记（冻结锚 · 判据面）
    ("FZ_B", 154), // FZ 副登记（冻结锚 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2501reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2501reg_COUNT {
        if u2501reg[i].0 == key {
            return Some(u2501reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2501reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2501reg_COUNT {
        s = s.wrapping_add(u2501reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2501Ledger（冻结接口 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 冻结接口 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2501Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2501Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa3c0, 0xa3cf]。
pub const fn u2501_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2501Code::ALL.len() {
        let c = u2501Code::ALL[i].code();
        if c < 0xa3c0 || c > 0xa3cf {
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
fn u2501_guard_seg() -> bool {
    u2501_segment_guard()
}

/// UNX-F2501 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2501_core_tbl() -> bool {
    let mut ok = u2501reg_COUNT == 24;
    if ok { ok = u2501reg_get("IF_A") == Some(7) && u2501reg_get("IF_B") == Some(33); }
    if ok { ok = u2501reg_get("FZ_A") == Some(84); }
    if ok { ok = u2501reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2501reg_sum() == 1668; }
    ok
}

/// UNX-F2501 守恒账三操作确定性效果。
fn check_u2501_core_ldg() -> bool {
    let mut ld = Mxu2501Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2501_checks() -> CheckSet {
    let mut set = CheckSet::new("U2501");
    set.add(
        "u2501-seg-guard",
        u2501_guard_seg(),
        if u2501_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2501-core-tbl",
        check_u2501_core_tbl(),
        if check_u2501_core_tbl() { "" } else { "UNX-F2501 core table mismatch" },
    );
    set.add(
        "u2501-core-ldg",
        check_u2501_core_ldg(),
        if check_u2501_core_ldg() { "" } else { "UNX-F2501 core ledger mismatch" },
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
    fn u2501_all_checks_green() {
        let set = run_u2501_checks();
        assert!(set.all_passed(), "U2501 domain checks must be green");
    }

    #[test]
    fn u2501_segment_guard_holds() {
        assert!(u2501_segment_guard());
    }
}
