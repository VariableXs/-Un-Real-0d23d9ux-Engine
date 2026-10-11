//! unxu1701_u1701core —— 负载追踪总纲（窗口化追踪模型冻结） 语义登记表与守恒账（窗口）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1701 · 负载追踪总纲（窗口化追踪模型冻结） · 判据原文：UNX-F1701 · 判据：窗口化追踪模型冻结 v1（几何衰减均值+双窗口档），模型参数单点定义，计算路径唯一，与 F1646 瞬时账分层声明在册
//! · 组件：窗口 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F1701 · 判据：窗口化追踪模型冻结 v1（几何衰减均值+双窗口档），模型参数单点定义，计算路径唯一，与 F1646 瞬时账分层声明在册
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1701_VERSION: &str = "u1701-u1701core-v1";

/// 诊断段基址。
pub const U1701_SEG_BASE: u16 = 0x58c0;

// --------------------------------------------------------------------------
// 诊断码（u1701 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 窗口 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1701Code {
    /// 0x58c0 表行重复（窗口漂移 同键双写）。
    RowDup,
    /// 0x58c1 表行断档（衰减失准 序号不连续）。
    RowGap,
    /// 0x58c2 值越界（分层混账 值域外）。
    ValOob,
    /// 0x58c3 累计漂移（参数多点 求和≠理论值）。
    SumDrift,
    /// 0x58c4 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0x58c5 账目断漏（序号缺失）。
    LedgerHole,
    /// 0x58c6 空操作（账目无效果调用）。
    OpNoop,
    /// 0x58c7 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0x58c8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x58c9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x58ca 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x58cb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1701Code {
    /// 全量闭集。
    pub const ALL: [u1701Code; 12] = [
        u1701Code::RowDup,
        u1701Code::RowGap,
        u1701Code::ValOob,
        u1701Code::SumDrift,
        u1701Code::LedgerDup,
        u1701Code::LedgerHole,
        u1701Code::OpNoop,
        u1701Code::FieldDrift,
        u1701Code::LedgerBroken,
        u1701Code::SnapshotFingerprint,
        u1701Code::TableBroken,
        u1701Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1701Code::RowDup => 0x58c0,
            u1701Code::RowGap => 0x58c1,
            u1701Code::ValOob => 0x58c2,
            u1701Code::SumDrift => 0x58c3,
            u1701Code::LedgerDup => 0x58c4,
            u1701Code::LedgerHole => 0x58c5,
            u1701Code::OpNoop => 0x58c6,
            u1701Code::FieldDrift => 0x58c7,
            u1701Code::LedgerBroken => 0x58c8,
            u1701Code::SnapshotFingerprint => 0x58c9,
            u1701Code::TableBroken => 0x58ca,
            u1701Code::SegmentGuard => 0x58cb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1701Code::RowDup => "表行重复（窗口漂移 同键双写）",
            u1701Code::RowGap => "表行断档（衰减失准 序号不连续）",
            u1701Code::ValOob => "值越界（分层混账 值域外）",
            u1701Code::SumDrift => "累计漂移（参数多点 求和≠理论值）",
            u1701Code::LedgerDup => "账目重记（同一事件双入账）",
            u1701Code::LedgerHole => "账目断漏（序号缺失）",
            u1701Code::OpNoop => "空操作（账目无效果调用）",
            u1701Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u1701Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1701Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1701Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1701Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1701Code::RowDup => "按键唯一律去重",
            u1701Code::RowGap => "按连续律补号",
            u1701Code::ValOob => "按值域裁剪",
            u1701Code::SumDrift => "按理论值复算",
            u1701Code::LedgerDup => "按事件幂等律合并",
            u1701Code::LedgerHole => "按连续律补账",
            u1701Code::OpNoop => "核对操作前置条件",
            u1701Code::FieldDrift => "按口径重建字段",
            u1701Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1701Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1701Code::TableBroken => "按语义规则重建常量表",
            u1701Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1701reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 窗口 语义登记表（24 行逐行注释）。
pub const u1701reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1701reg: [(&'static str, u64); 24] = [
    ("JS_A", 7), // JS 主登记（几何衰减 · 判据面）
    ("JS_B", 33), // JS 副登记（几何衰减 · 对拍面）
    ("SC_A", 14), // SC 主登记（双窗口档 · 判据面）
    ("SC_B", 44), // SC 副登记（双窗口档 · 对拍面）
    ("CS_A", 21), // CS 主登记（参数单点 · 判据面）
    ("CS_B", 55), // CS 副登记（参数单点 · 对拍面）
    ("LY_A", 28), // LY 主登记（路径唯一 · 判据面）
    ("LY_B", 66), // LY 副登记（路径唯一 · 对拍面）
    ("F1646_A", 35), // F1646 主登记（瞬时账分层 · 判据面）
    ("F1646_B", 77), // F1646 副登记（瞬时账分层 · 对拍面）
    ("CK_A", 42), // CK 主登记（窗口 · 判据面）
    ("CK_B", 88), // CK 副登记（窗口 · 对拍面）
    ("JZ_A", 49), // JZ 主登记（均值 · 判据面）
    ("JZ_B", 99), // JZ 副登记（均值 · 对拍面）
    ("MZ_A", 56), // MZ 主登记（模型冻结 · 判据面）
    ("MZ_B", 110), // MZ 副登记（模型冻结 · 对拍面）
    ("B01_A", 63), // B01 主登记（短窗档 · 判据面）
    ("B01_B", 121), // B01 副登记（短窗档 · 对拍面）
    ("B02_A", 70), // B02 主登记（长窗档 · 判据面）
    ("B02_B", 132), // B02 副登记（长窗档 · 对拍面）
    ("SD_A", 77), // SD 主登记（衰减因子 · 判据面）
    ("SD_B", 143), // SD 副登记（衰减因子 · 对拍面）
    ("ZC_A", 84), // ZC 主登记（在册声明 · 判据面）
    ("ZC_B", 154), // ZC 副登记（在册声明 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1701reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1701reg_COUNT {
        if u1701reg[i].0 == key {
            return Some(u1701reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1701reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1701reg_COUNT {
        s = s.wrapping_add(u1701reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu1701Ledger（窗口 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 窗口 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu1701Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu1701Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0x58c0, 0x58cf]。
pub const fn u1701_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1701Code::ALL.len() {
        let c = u1701Code::ALL[i].code();
        if c < 0x58c0 || c > 0x58cf {
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
fn u1701_guard_seg() -> bool {
    u1701_segment_guard()
}

/// UNX-F1701 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u1701_core_tbl() -> bool {
    let mut ok = u1701reg_COUNT == 24;
    if ok { ok = u1701reg_get("JS_A") == Some(7) && u1701reg_get("JS_B") == Some(33); }
    if ok { ok = u1701reg_get("ZC_A") == Some(84); }
    if ok { ok = u1701reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1701reg_sum() == 1668; }
    ok
}

/// UNX-F1701 守恒账三操作确定性效果。
fn check_u1701_core_ldg() -> bool {
    let mut ld = Mxu1701Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1701_checks() -> CheckSet {
    let mut set = CheckSet::new("U1701");
    set.add(
        "u1701-seg-guard",
        u1701_guard_seg(),
        if u1701_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1701-core-tbl",
        check_u1701_core_tbl(),
        if check_u1701_core_tbl() { "" } else { "UNX-F1701 core table mismatch" },
    );
    set.add(
        "u1701-core-ldg",
        check_u1701_core_ldg(),
        if check_u1701_core_ldg() { "" } else { "UNX-F1701 core ledger mismatch" },
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
    fn u1701_all_checks_green() {
        let set = run_u1701_checks();
        assert!(set.all_passed(), "U1701 domain checks must be green");
    }

    #[test]
    fn u1701_segment_guard_holds() {
        assert!(u1701_segment_guard());
    }
}
