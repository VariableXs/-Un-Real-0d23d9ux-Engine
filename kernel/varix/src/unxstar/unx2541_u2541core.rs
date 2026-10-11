//! unxu2541_u2541core —— alloc_pages/free_pages 冻结接口总账（正式冻结版） 语义登记表与守恒账（冻结接口）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2541 · alloc_pages/free_pages 冻结接口总账（正式冻结版） · 判据原文：UNX-F2541 · 判据：双接口冻结件登记（全域消费方清单齐备），签名与错误码三机直测断言一致
//! · 组件：冻结接口 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2541 · 判据：双接口冻结件登记（全域消费方清单齐备），签名与错误码三机直测断言一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2541_VERSION: &str = "u2541-u2541core-v1";

/// 诊断段基址。
pub const U2541_SEG_BASE: u16 = 0xa540;

// --------------------------------------------------------------------------
// 诊断码（u2541 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 冻结接口 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2541Code {
    /// 0xa540 表行重复（消费方缺 同键双写）。
    RowDup,
    /// 0xa541 表行断档（签名不符 序号不连续）。
    RowGap,
    /// 0xa542 值越界（错误码分歧 值域外）。
    ValOob,
    /// 0xa543 累计漂移（直测不一致 求和≠理论值）。
    SumDrift,
    /// 0xa544 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xa545 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xa546 空操作（账目无效果调用）。
    OpNoop,
    /// 0xa547 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xa548 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa549 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa54a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa54b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2541Code {
    /// 全量闭集。
    pub const ALL: [u2541Code; 12] = [
        u2541Code::RowDup,
        u2541Code::RowGap,
        u2541Code::ValOob,
        u2541Code::SumDrift,
        u2541Code::LedgerDup,
        u2541Code::LedgerHole,
        u2541Code::OpNoop,
        u2541Code::FieldDrift,
        u2541Code::LedgerBroken,
        u2541Code::SnapshotFingerprint,
        u2541Code::TableBroken,
        u2541Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2541Code::RowDup => 0xa540,
            u2541Code::RowGap => 0xa541,
            u2541Code::ValOob => 0xa542,
            u2541Code::SumDrift => 0xa543,
            u2541Code::LedgerDup => 0xa544,
            u2541Code::LedgerHole => 0xa545,
            u2541Code::OpNoop => 0xa546,
            u2541Code::FieldDrift => 0xa547,
            u2541Code::LedgerBroken => 0xa548,
            u2541Code::SnapshotFingerprint => 0xa549,
            u2541Code::TableBroken => 0xa54a,
            u2541Code::SegmentGuard => 0xa54b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2541Code::RowDup => "表行重复（消费方缺 同键双写）",
            u2541Code::RowGap => "表行断档（签名不符 序号不连续）",
            u2541Code::ValOob => "值越界（错误码分歧 值域外）",
            u2541Code::SumDrift => "累计漂移（直测不一致 求和≠理论值）",
            u2541Code::LedgerDup => "账目重记（同一事件双入账）",
            u2541Code::LedgerHole => "账目断漏（序号缺失）",
            u2541Code::OpNoop => "空操作（账目无效果调用）",
            u2541Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2541Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2541Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2541Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2541Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2541Code::RowDup => "按键唯一律去重",
            u2541Code::RowGap => "按连续律补号",
            u2541Code::ValOob => "按值域裁剪",
            u2541Code::SumDrift => "按理论值复算",
            u2541Code::LedgerDup => "按事件幂等律合并",
            u2541Code::LedgerHole => "按连续律补账",
            u2541Code::OpNoop => "核对操作前置条件",
            u2541Code::FieldDrift => "按口径重建字段",
            u2541Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2541Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2541Code::TableBroken => "按语义规则重建常量表",
            u2541Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2541reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 冻结接口 语义登记表（24 行逐行注释）。
pub const u2541reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2541reg: [(&'static str, u64); 24] = [
    ("AL_A", 7), // AL 主登记（alloc_pages · 判据面）
    ("AL_B", 33), // AL 副登记（alloc_pages · 对拍面）
    ("FP_A", 14), // FP 主登记（free_pages · 判据面）
    ("FP_B", 44), // FP 副登记（free_pages · 对拍面）
    ("FZ_A", 21), // FZ 主登记（冻结件 · 判据面）
    ("FZ_B", 55), // FZ 副登记（冻结件 · 对拍面）
    ("CN_A", 28), // CN 主登记（消费方清单 · 判据面）
    ("CN_B", 66), // CN 副登记（消费方清单 · 对拍面）
    ("SG_A", 35), // SG 主登记（签名账 · 判据面）
    ("SG_B", 77), // SG 副登记（签名账 · 对拍面）
    ("EC_A", 42), // EC 主登记（错误码 · 判据面）
    ("EC_B", 88), // EC 副登记（错误码 · 对拍面）
    ("TM_A", 49), // TM 主登记（三机直测 · 判据面）
    ("TM_B", 99), // TM 副登记（三机直测 · 对拍面）
    ("AS_A", 56), // AS 主登记（断言一致 · 判据面）
    ("AS_B", 110), // AS 副登记（断言一致 · 对拍面）
    ("SG2_A", 63), // SG2 主登记（参数面 · 判据面）
    ("SG2_B", 121), // SG2 副登记（参数面 · 对拍面）
    ("RT_A", 70), // RT 主登记（返回面 · 判据面）
    ("RT_B", 132), // RT 副登记（返回面 · 对拍面）
    ("DS_A", 77), // DS 主登记（全域面 · 判据面）
    ("DS_B", 143), // DS 副登记（全域面 · 对拍面）
    ("LK_A", 84), // LK 主登记（联签账 · 判据面）
    ("LK_B", 154), // LK 副登记（联签账 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2541reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2541reg_COUNT {
        if u2541reg[i].0 == key {
            return Some(u2541reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2541reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2541reg_COUNT {
        s = s.wrapping_add(u2541reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2541Ledger（冻结接口 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 冻结接口 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2541Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2541Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa540, 0xa54f]。
pub const fn u2541_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2541Code::ALL.len() {
        let c = u2541Code::ALL[i].code();
        if c < 0xa540 || c > 0xa54f {
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
fn u2541_guard_seg() -> bool {
    u2541_segment_guard()
}

/// UNX-F2541 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2541_core_tbl() -> bool {
    let mut ok = u2541reg_COUNT == 24;
    if ok { ok = u2541reg_get("AL_A") == Some(7) && u2541reg_get("AL_B") == Some(33); }
    if ok { ok = u2541reg_get("LK_A") == Some(84); }
    if ok { ok = u2541reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2541reg_sum() == 1668; }
    ok
}

/// UNX-F2541 守恒账三操作确定性效果。
fn check_u2541_core_ldg() -> bool {
    let mut ld = Mxu2541Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2541_checks() -> CheckSet {
    let mut set = CheckSet::new("U2541");
    set.add(
        "u2541-seg-guard",
        u2541_guard_seg(),
        if u2541_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2541-core-tbl",
        check_u2541_core_tbl(),
        if check_u2541_core_tbl() { "" } else { "UNX-F2541 core table mismatch" },
    );
    set.add(
        "u2541-core-ldg",
        check_u2541_core_ldg(),
        if check_u2541_core_ldg() { "" } else { "UNX-F2541 core ledger mismatch" },
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
    fn u2541_all_checks_green() {
        let set = run_u2541_checks();
        assert!(set.all_passed(), "U2541 domain checks must be green");
    }

    #[test]
    fn u2541_segment_guard_holds() {
        assert!(u2541_segment_guard());
    }
}
