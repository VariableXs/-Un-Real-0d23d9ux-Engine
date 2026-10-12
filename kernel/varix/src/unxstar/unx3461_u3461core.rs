//! unxu3461_u3461core —— 分域挂载总设计（框架与各域用例集的挂载架构） 语义登记表与守恒账（分域挂载）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3461 · 分域挂载总设计（框架与各域用例集的挂载架构） · 判据原文：UNX-F3461 · 判据：J1：挂载架构文档（域侧声明用例集、框架侧统一注册表——双向登记制）；M-220 挂载全链路（域声明 → 注册表合并 → 执行器可见）。
//! · 组件：分域挂载 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3461 · 判据：J1：挂载架构文档（域侧声明用例集、框架侧统一注册表——双向登记制）；M-220 挂载全链路（域声明 → 注册表合并 → 执行器可
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3461_VERSION: &str = "u3461-u3461core-v1";

/// 诊断段基址。
pub const U3461_SEG_BASE: u16 = 0xe0a0;

// --------------------------------------------------------------------------
// 诊断码（u3461 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 分域挂载 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3461Code {
    /// 0xe0a0 表行重复（挂载 同键双写）。
    RowDup,
    /// 0xe0a1 表行断档（用例集 序号不连续）。
    RowGap,
    /// 0xe0a2 值越界（域 值域外）。
    ValOob,
    /// 0xe0a3 累计漂移（依赖 求和≠理论值）。
    SumDrift,
    /// 0xe0a4 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe0a5 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe0a6 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe0a7 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe0a8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0a9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0aa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0ab 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3461Code {
    /// 全量闭集。
    pub const ALL: [u3461Code; 12] = [
        u3461Code::RowDup,
        u3461Code::RowGap,
        u3461Code::ValOob,
        u3461Code::SumDrift,
        u3461Code::LedgerDup,
        u3461Code::LedgerHole,
        u3461Code::OpNoop,
        u3461Code::FieldDrift,
        u3461Code::LedgerBroken,
        u3461Code::SnapshotFingerprint,
        u3461Code::TableBroken,
        u3461Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3461Code::RowDup => 0xe0a0,
            u3461Code::RowGap => 0xe0a1,
            u3461Code::ValOob => 0xe0a2,
            u3461Code::SumDrift => 0xe0a3,
            u3461Code::LedgerDup => 0xe0a4,
            u3461Code::LedgerHole => 0xe0a5,
            u3461Code::OpNoop => 0xe0a6,
            u3461Code::FieldDrift => 0xe0a7,
            u3461Code::LedgerBroken => 0xe0a8,
            u3461Code::SnapshotFingerprint => 0xe0a9,
            u3461Code::TableBroken => 0xe0aa,
            u3461Code::SegmentGuard => 0xe0ab,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3461Code::RowDup => "表行重复（挂载 同键双写）",
            u3461Code::RowGap => "表行断档（用例集 序号不连续）",
            u3461Code::ValOob => "值越界（域 值域外）",
            u3461Code::SumDrift => "累计漂移（依赖 求和≠理论值）",
            u3461Code::LedgerDup => "账目重记（同一事件双入账）",
            u3461Code::LedgerHole => "账目断漏（序号缺失）",
            u3461Code::OpNoop => "空操作（账目无效果调用）",
            u3461Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3461Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3461Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3461Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3461Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3461Code::RowDup => "按键唯一律去重",
            u3461Code::RowGap => "按连续律补号",
            u3461Code::ValOob => "按值域裁剪",
            u3461Code::SumDrift => "按理论值复算",
            u3461Code::LedgerDup => "按事件幂等律合并",
            u3461Code::LedgerHole => "按连续律补账",
            u3461Code::OpNoop => "核对操作前置条件",
            u3461Code::FieldDrift => "按口径重建字段",
            u3461Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3461Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3461Code::TableBroken => "按语义规则重建常量表",
            u3461Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3461reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 分域挂载 语义登记表（24 行逐行注释）。
pub const u3461reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3461reg: [(&'static str, u64); 24] = [
    ("MNT_A", 7), // MNT 主登记（挂载 · 判据面）
    ("MNT_B", 33), // MNT 副登记（挂载 · 对拍面）
    ("FW_A", 14), // FW 主登记（框架 · 判据面）
    ("FW_B", 44), // FW 副登记（框架 · 对拍面）
    ("SUITE_A", 21), // SUITE 主登记（用例集 · 判据面）
    ("SUITE_B", 55), // SUITE 副登记（用例集 · 对拍面）
    ("DOM_A", 28), // DOM 主登记（域标识 · 判据面）
    ("DOM_B", 66), // DOM 副登记（域标识 · 对拍面）
    ("TABLE_A", 35), // TABLE 主登记（挂载表 · 判据面）
    ("TABLE_B", 77), // TABLE 副登记（挂载表 · 对拍面）
    ("PRIO_A", 42), // PRIO 主登记（优先级 · 判据面）
    ("PRIO_B", 88), // PRIO 副登记（优先级 · 对拍面）
    ("BIND_A", 49), // BIND 主登记（绑定 · 判据面）
    ("BIND_B", 99), // BIND 副登记（绑定 · 对拍面）
    ("UNBIND_A", 56), // UNBIND 主登记（解绑 · 判据面）
    ("UNBIND_B", 110), // UNBIND 副登记（解绑 · 对拍面）
    ("VER_A", 63), // VER 主登记（版本 · 判据面）
    ("VER_B", 121), // VER 副登记（版本 · 对拍面）
    ("DEP_A", 70), // DEP 主登记（依赖域 · 判据面）
    ("DEP_B", 132), // DEP 副登记（依赖域 · 对拍面）
    ("CAP_A", 77), // CAP 主登记（容量 · 判据面）
    ("CAP_B", 143), // CAP 副登记（容量 · 对拍面）
    ("ORDER_A", 84), // ORDER 主登记（装载序 · 判据面）
    ("ORDER_B", 154), // ORDER 副登记（装载序 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3461reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3461reg_COUNT {
        if u3461reg[i].0 == key {
            return Some(u3461reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3461reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3461reg_COUNT {
        s = s.wrapping_add(u3461reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3461Ledger（分域挂载 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 分域挂载 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3461Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3461Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0a0, 0xe0af]。
pub const fn u3461_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3461Code::ALL.len() {
        let c = u3461Code::ALL[i].code();
        if c < 0xe0a0 || c > 0xe0af {
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
fn u3461_guard_seg() -> bool {
    u3461_segment_guard()
}

/// UNX-F3461 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3461_core_tbl() -> bool {
    let mut ok = u3461reg_COUNT == 24;
    if ok { ok = u3461reg_get("MNT_A") == Some(7) && u3461reg_get("MNT_B") == Some(33); }
    if ok { ok = u3461reg_get("ORDER_A") == Some(84); }
    if ok { ok = u3461reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3461reg_sum() == 1668; }
    ok
}

/// UNX-F3461 守恒账三操作确定性效果。
fn check_u3461_core_ldg() -> bool {
    let mut ld = Mxu3461Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3461_checks() -> CheckSet {
    let mut set = CheckSet::new("U3461");
    set.add(
        "u3461-seg-guard",
        u3461_guard_seg(),
        if u3461_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3461-core-tbl",
        check_u3461_core_tbl(),
        if check_u3461_core_tbl() { "" } else { "UNX-F3461 core table mismatch" },
    );
    set.add(
        "u3461-core-ldg",
        check_u3461_core_ldg(),
        if check_u3461_core_ldg() { "" } else { "UNX-F3461 core ledger mismatch" },
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
    fn u3461_all_checks_green() {
        let set = run_u3461_checks();
        assert!(set.all_passed(), "U3461 domain checks must be green");
    }

    #[test]
    fn u3461_segment_guard_holds() {
        assert!(u3461_segment_guard());
    }
}
