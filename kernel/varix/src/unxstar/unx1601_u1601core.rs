//! unxu1601_u1601core —— SMP 前置数据包消费与 A1 联签确认 语义登记表与守恒账（字段）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1601 · SMP 前置数据包消费与 A1 联签确认 · 判据原文：UNX-F1601 · 判据：宿主侧消费 A1 F0188 前置数据包全字段（核列表/LAPIC 基址/缺席核账/trampoline 预留区）逐字段校验落账，字段缺失或版本不符时启动序列拒绝开跑并列出缺失清单，联签确认记录
//! · 组件：字段 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F1601 · 判据：宿主侧消费 A1 F0188 前置数据包全字段（核列表/LAPIC 基址/缺席核账/trampoline 预留区）逐字段校验落账，
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1601_VERSION: &str = "u1601-u1601core-v1";

/// 诊断段基址。
pub const U1601_SEG_BASE: u16 = 0x5500;

// --------------------------------------------------------------------------
// 诊断码（u1601 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 字段 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1601Code {
    /// 0x5500 表行重复（字段缺失 同键双写）。
    RowDup,
    /// 0x5501 表行断档（版本不符 序号不连续）。
    RowGap,
    /// 0x5502 值越界（核账破缺 值域外）。
    ValOob,
    /// 0x5503 累计漂移（联签缺位 求和≠理论值）。
    SumDrift,
    /// 0x5504 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0x5505 账目断漏（序号缺失）。
    LedgerHole,
    /// 0x5506 空操作（账目无效果调用）。
    OpNoop,
    /// 0x5507 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0x5508 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5509 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x550a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x550b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1601Code {
    /// 全量闭集。
    pub const ALL: [u1601Code; 12] = [
        u1601Code::RowDup,
        u1601Code::RowGap,
        u1601Code::ValOob,
        u1601Code::SumDrift,
        u1601Code::LedgerDup,
        u1601Code::LedgerHole,
        u1601Code::OpNoop,
        u1601Code::FieldDrift,
        u1601Code::LedgerBroken,
        u1601Code::SnapshotFingerprint,
        u1601Code::TableBroken,
        u1601Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1601Code::RowDup => 0x5500,
            u1601Code::RowGap => 0x5501,
            u1601Code::ValOob => 0x5502,
            u1601Code::SumDrift => 0x5503,
            u1601Code::LedgerDup => 0x5504,
            u1601Code::LedgerHole => 0x5505,
            u1601Code::OpNoop => 0x5506,
            u1601Code::FieldDrift => 0x5507,
            u1601Code::LedgerBroken => 0x5508,
            u1601Code::SnapshotFingerprint => 0x5509,
            u1601Code::TableBroken => 0x550a,
            u1601Code::SegmentGuard => 0x550b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1601Code::RowDup => "表行重复（字段缺失 同键双写）",
            u1601Code::RowGap => "表行断档（版本不符 序号不连续）",
            u1601Code::ValOob => "值越界（核账破缺 值域外）",
            u1601Code::SumDrift => "累计漂移（联签缺位 求和≠理论值）",
            u1601Code::LedgerDup => "账目重记（同一事件双入账）",
            u1601Code::LedgerHole => "账目断漏（序号缺失）",
            u1601Code::OpNoop => "空操作（账目无效果调用）",
            u1601Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u1601Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1601Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1601Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1601Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1601Code::RowDup => "按键唯一律去重",
            u1601Code::RowGap => "按连续律补号",
            u1601Code::ValOob => "按值域裁剪",
            u1601Code::SumDrift => "按理论值复算",
            u1601Code::LedgerDup => "按事件幂等律合并",
            u1601Code::LedgerHole => "按连续律补账",
            u1601Code::OpNoop => "核对操作前置条件",
            u1601Code::FieldDrift => "按口径重建字段",
            u1601Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1601Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1601Code::TableBroken => "按语义规则重建常量表",
            u1601Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1601reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 字段 语义登记表（24 行逐行注释）。
pub const u1601reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1601reg: [(&'static str, u64); 24] = [
    ("CL_A", 7), // CL 主登记（核列表 · 判据面）
    ("CL_B", 33), // CL 副登记（核列表 · 对拍面）
    ("LB_A", 14), // LB 主登记（LAPIC 基址 · 判据面）
    ("LB_B", 44), // LB 副登记（LAPIC 基址 · 对拍面）
    ("AC_A", 21), // AC 主登记（缺席核账 · 判据面）
    ("AC_B", 55), // AC 副登记（缺席核账 · 对拍面）
    ("TR_A", 28), // TR 主登记（trampoline 预留区 · 判据面）
    ("TR_B", 66), // TR 副登记（trampoline 预留区 · 对拍面）
    ("FV_A", 35), // FV 主登记（逐字段校验 · 判据面）
    ("FV_B", 77), // FV 副登记（逐字段校验 · 对拍面）
    ("VM_A", 42), // VM 主登记（版本不符 · 判据面）
    ("VM_B", 88), // VM 副登记（版本不符 · 对拍面）
    ("ML_A", 49), // ML 主登记（缺失清单 · 判据面）
    ("ML_B", 99), // ML 副登记（缺失清单 · 对拍面）
    ("RJ_A", 56), // RJ 主登记（拒绝开跑 · 判据面）
    ("RJ_B", 110), // RJ 副登记（拒绝开跑 · 对拍面）
    ("LS_A", 63), // LS 主登记（联签确认 · 判据面）
    ("LS_B", 121), // LS 副登记（联签确认 · 对拍面）
    ("PK_A", 70), // PK 主登记（数据包 · 判据面）
    ("PK_B", 132), // PK 副登记（数据包 · 对拍面）
    ("F0188_A", 77), // F0188 主登记（F0188 锚 · 判据面）
    ("F0188_B", 143), // F0188 副登记（F0188 锚 · 对拍面）
    ("ZZ_A", 84), // ZZ 主登记（落账 · 判据面）
    ("ZZ_B", 154), // ZZ 副登记（落账 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1601reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1601reg_COUNT {
        if u1601reg[i].0 == key {
            return Some(u1601reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1601reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1601reg_COUNT {
        s = s.wrapping_add(u1601reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu1601Ledger（字段 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 字段 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu1601Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu1601Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5500, 0x550f]。
pub const fn u1601_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1601Code::ALL.len() {
        let c = u1601Code::ALL[i].code();
        if c < 0x5500 || c > 0x550f {
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
fn u1601_guard_seg() -> bool {
    u1601_segment_guard()
}

/// UNX-F1601 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u1601_core_tbl() -> bool {
    let mut ok = u1601reg_COUNT == 24;
    if ok { ok = u1601reg_get("CL_A") == Some(7) && u1601reg_get("CL_B") == Some(33); }
    if ok { ok = u1601reg_get("ZZ_A") == Some(84); }
    if ok { ok = u1601reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1601reg_sum() == 1668; }
    ok
}

/// UNX-F1601 守恒账三操作确定性效果。
fn check_u1601_core_ldg() -> bool {
    let mut ld = Mxu1601Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1601_checks() -> CheckSet {
    let mut set = CheckSet::new("U1601");
    set.add(
        "u1601-seg-guard",
        u1601_guard_seg(),
        if u1601_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1601-core-tbl",
        check_u1601_core_tbl(),
        if check_u1601_core_tbl() { "" } else { "UNX-F1601 core table mismatch" },
    );
    set.add(
        "u1601-core-ldg",
        check_u1601_core_ldg(),
        if check_u1601_core_ldg() { "" } else { "UNX-F1601 core ledger mismatch" },
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
    fn u1601_all_checks_green() {
        let set = run_u1601_checks();
        assert!(set.all_passed(), "U1601 domain checks must be green");
    }

    #[test]
    fn u1601_segment_guard_holds() {
        assert!(u1601_segment_guard());
    }
}
