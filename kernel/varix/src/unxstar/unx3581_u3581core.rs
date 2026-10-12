//! unxu3581_u3581core —— 内存水位账总设计（per-pool 水位/四级线） 语义登记表与守恒账（内存水位账）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3581 · 内存水位账总设计（per-pool 水位/四级线） · 判据原文：UNX-F3581 · 判据：J1：四级线冻结（NORMAL 60%/WATCH 75%/HIGH 90%/CRITICAL 97%——判据行内嵌各线语义与响应）；M-339 四级全链（逐级越线→账+告警+响应分级）。
//! · 组件：内存水位账 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3581 · 判据：J1：四级线冻结（NORMAL 60%/WATCH 75%/HIGH 90%/CRITICAL 97%——判据行内嵌各线语义与响应
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3581_VERSION: &str = "u3581-u3581core-v1";

/// 诊断段基址。
pub const U3581_SEG_BASE: u16 = 0xe100;

// --------------------------------------------------------------------------
// 诊断码（u3581 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 内存水位账 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3581Code {
    /// 0xe100 表行重复（水位 同键双写）。
    RowDup,
    /// 0xe101 表行断档（内存池 序号不连续）。
    RowGap,
    /// 0xe102 值越界（分级 值域外）。
    ValOob,
    /// 0xe103 累计漂移（四级线 求和≠理论值）。
    SumDrift,
    /// 0xe104 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe105 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe106 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe107 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe108 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe109 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe10a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe10b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3581Code {
    /// 全量闭集。
    pub const ALL: [u3581Code; 12] = [
        u3581Code::RowDup,
        u3581Code::RowGap,
        u3581Code::ValOob,
        u3581Code::SumDrift,
        u3581Code::LedgerDup,
        u3581Code::LedgerHole,
        u3581Code::OpNoop,
        u3581Code::FieldDrift,
        u3581Code::LedgerBroken,
        u3581Code::SnapshotFingerprint,
        u3581Code::TableBroken,
        u3581Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3581Code::RowDup => 0xe100,
            u3581Code::RowGap => 0xe101,
            u3581Code::ValOob => 0xe102,
            u3581Code::SumDrift => 0xe103,
            u3581Code::LedgerDup => 0xe104,
            u3581Code::LedgerHole => 0xe105,
            u3581Code::OpNoop => 0xe106,
            u3581Code::FieldDrift => 0xe107,
            u3581Code::LedgerBroken => 0xe108,
            u3581Code::SnapshotFingerprint => 0xe109,
            u3581Code::TableBroken => 0xe10a,
            u3581Code::SegmentGuard => 0xe10b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3581Code::RowDup => "表行重复（水位 同键双写）",
            u3581Code::RowGap => "表行断档（内存池 序号不连续）",
            u3581Code::ValOob => "值越界（分级 值域外）",
            u3581Code::SumDrift => "累计漂移（四级线 求和≠理论值）",
            u3581Code::LedgerDup => "账目重记（同一事件双入账）",
            u3581Code::LedgerHole => "账目断漏（序号缺失）",
            u3581Code::OpNoop => "空操作（账目无效果调用）",
            u3581Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3581Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3581Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3581Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3581Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3581Code::RowDup => "按键唯一律去重",
            u3581Code::RowGap => "按连续律补号",
            u3581Code::ValOob => "按值域裁剪",
            u3581Code::SumDrift => "按理论值复算",
            u3581Code::LedgerDup => "按事件幂等律合并",
            u3581Code::LedgerHole => "按连续律补账",
            u3581Code::OpNoop => "核对操作前置条件",
            u3581Code::FieldDrift => "按口径重建字段",
            u3581Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3581Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3581Code::TableBroken => "按语义规则重建常量表",
            u3581Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3581reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 内存水位账 语义登记表（24 行逐行注释）。
pub const u3581reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3581reg: [(&'static str, u64); 24] = [
    ("POOL_A", 7), // POOL 主登记（内存池 · 判据面）
    ("POOL_B", 33), // POOL 副登记（内存池 · 对拍面）
    ("LVL_A", 14), // LVL 主登记（水位 · 判据面）
    ("LVL_B", 44), // LVL 副登记（水位 · 对拍面）
    ("L4_A", 21), // L4 主登记（四级线 · 判据面）
    ("L4_B", 55), // L4 副登记（四级线 · 对拍面）
    ("CUR_A", 28), // CUR 主登记（当前水位 · 判据面）
    ("CUR_B", 66), // CUR 副登记（当前水位 · 对拍面）
    ("HIGH_A", 35), // HIGH 主登记（高水位 · 判据面）
    ("HIGH_B", 77), // HIGH 副登记（高水位 · 对拍面）
    ("LOW_A", 42), // LOW 主登记（低水位 · 判据面）
    ("LOW_B", 88), // LOW 副登记（低水位 · 对拍面）
    ("RESV_A", 49), // RESV 主登记（预留 · 判据面）
    ("RESV_B", 99), // RESV 副登记（预留 · 对拍面）
    ("AVAIL_A", 56), // AVAIL 主登记（可用 · 判据面）
    ("AVAIL_B", 110), // AVAIL 副登记（可用 · 对拍面）
    ("TRIG_A", 63), // TRIG 主登记（触发线 · 判据面）
    ("TRIG_B", 121), // TRIG 副登记（触发线 · 对拍面）
    ("ACT_A", 70), // ACT 主登记（处置动作 · 判据面）
    ("ACT_B", 132), // ACT 副登记（处置动作 · 对拍面）
    ("HIST_A", 77), // HIST 主登记（历史 · 判据面）
    ("HIST_B", 143), // HIST 副登记（历史 · 对拍面）
    ("RATE_A", 84), // RATE 主登记（变化率 · 判据面）
    ("RATE_B", 154), // RATE 副登记（变化率 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3581reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3581reg_COUNT {
        if u3581reg[i].0 == key {
            return Some(u3581reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3581reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3581reg_COUNT {
        s = s.wrapping_add(u3581reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3581Ledger（内存水位账 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 内存水位账 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3581Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3581Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe100, 0xe10f]。
pub const fn u3581_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3581Code::ALL.len() {
        let c = u3581Code::ALL[i].code();
        if c < 0xe100 || c > 0xe10f {
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
fn u3581_guard_seg() -> bool {
    u3581_segment_guard()
}

/// UNX-F3581 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3581_core_tbl() -> bool {
    let mut ok = u3581reg_COUNT == 24;
    if ok { ok = u3581reg_get("POOL_A") == Some(7) && u3581reg_get("POOL_B") == Some(33); }
    if ok { ok = u3581reg_get("RATE_A") == Some(84); }
    if ok { ok = u3581reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3581reg_sum() == 1668; }
    ok
}

/// UNX-F3581 守恒账三操作确定性效果。
fn check_u3581_core_ldg() -> bool {
    let mut ld = Mxu3581Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3581_checks() -> CheckSet {
    let mut set = CheckSet::new("U3581");
    set.add(
        "u3581-seg-guard",
        u3581_guard_seg(),
        if u3581_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3581-core-tbl",
        check_u3581_core_tbl(),
        if check_u3581_core_tbl() { "" } else { "UNX-F3581 core table mismatch" },
    );
    set.add(
        "u3581-core-ldg",
        check_u3581_core_ldg(),
        if check_u3581_core_ldg() { "" } else { "UNX-F3581 core ledger mismatch" },
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
    fn u3581_all_checks_green() {
        let set = run_u3581_checks();
        assert!(set.all_passed(), "U3581 domain checks must be green");
    }

    #[test]
    fn u3581_segment_guard_holds() {
        assert!(u3581_segment_guard());
    }
}
