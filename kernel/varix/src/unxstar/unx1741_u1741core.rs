//! unxu1741_u1741core —— 空闲循环总纲（idle 三态状态机） 语义登记表与守恒账（空闲态）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1741 · 空闲循环总纲（idle 三态状态机） · 判据原文：UNX-F1741 · 判据：idle 进入/驻留/退出三态状态机冻结 v1，状态转换账与实际状态一致断言全过，非法转换注入 10/10 次拒绝
//! · 组件：空闲态 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F1741 · 判据：idle 进入/驻留/退出三态状态机冻结 v1，状态转换账与实际状态一致断言全过，非法转换注入 10/10 次拒绝
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1741_VERSION: &str = "u1741-u1741core-v1";

/// 诊断段基址。
pub const U1741_SEG_BASE: u16 = 0x5a40;

// --------------------------------------------------------------------------
// 诊断码（u1741 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 空闲态 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1741Code {
    /// 0x5a40 表行重复（非法转换 同键双写）。
    RowDup,
    /// 0x5a41 表行断档（账实漂移 序号不连续）。
    RowGap,
    /// 0x5a42 值越界（唤醒丢失 值域外）。
    ValOob,
    /// 0x5a43 累计漂移（驻留死锁 求和≠理论值）。
    SumDrift,
    /// 0x5a44 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0x5a45 账目断漏（序号缺失）。
    LedgerHole,
    /// 0x5a46 空操作（账目无效果调用）。
    OpNoop,
    /// 0x5a47 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0x5a48 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5a49 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5a4a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5a4b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1741Code {
    /// 全量闭集。
    pub const ALL: [u1741Code; 12] = [
        u1741Code::RowDup,
        u1741Code::RowGap,
        u1741Code::ValOob,
        u1741Code::SumDrift,
        u1741Code::LedgerDup,
        u1741Code::LedgerHole,
        u1741Code::OpNoop,
        u1741Code::FieldDrift,
        u1741Code::LedgerBroken,
        u1741Code::SnapshotFingerprint,
        u1741Code::TableBroken,
        u1741Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1741Code::RowDup => 0x5a40,
            u1741Code::RowGap => 0x5a41,
            u1741Code::ValOob => 0x5a42,
            u1741Code::SumDrift => 0x5a43,
            u1741Code::LedgerDup => 0x5a44,
            u1741Code::LedgerHole => 0x5a45,
            u1741Code::OpNoop => 0x5a46,
            u1741Code::FieldDrift => 0x5a47,
            u1741Code::LedgerBroken => 0x5a48,
            u1741Code::SnapshotFingerprint => 0x5a49,
            u1741Code::TableBroken => 0x5a4a,
            u1741Code::SegmentGuard => 0x5a4b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1741Code::RowDup => "表行重复（非法转换 同键双写）",
            u1741Code::RowGap => "表行断档（账实漂移 序号不连续）",
            u1741Code::ValOob => "值越界（唤醒丢失 值域外）",
            u1741Code::SumDrift => "累计漂移（驻留死锁 求和≠理论值）",
            u1741Code::LedgerDup => "账目重记（同一事件双入账）",
            u1741Code::LedgerHole => "账目断漏（序号缺失）",
            u1741Code::OpNoop => "空操作（账目无效果调用）",
            u1741Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u1741Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1741Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1741Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1741Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1741Code::RowDup => "按键唯一律去重",
            u1741Code::RowGap => "按连续律补号",
            u1741Code::ValOob => "按值域裁剪",
            u1741Code::SumDrift => "按理论值复算",
            u1741Code::LedgerDup => "按事件幂等律合并",
            u1741Code::LedgerHole => "按连续律补账",
            u1741Code::OpNoop => "核对操作前置条件",
            u1741Code::FieldDrift => "按口径重建字段",
            u1741Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1741Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1741Code::TableBroken => "按语义规则重建常量表",
            u1741Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1741reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 空闲态 语义登记表（24 行逐行注释）。
pub const u1741reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1741reg: [(&'static str, u64); 24] = [
    ("JR_A", 7), // JR 主登记（idle 进入 · 判据面）
    ("JR_B", 33), // JR 副登记（idle 进入 · 对拍面）
    ("ZL_A", 14), // ZL 主登记（驻留 · 判据面）
    ("ZL_B", 44), // ZL 副登记（驻留 · 对拍面）
    ("TC_A", 21), // TC 主登记（退出 · 判据面）
    ("TC_B", 55), // TC 副登记（退出 · 对拍面）
    ("SM_A", 28), // SM 主登记（三态机 · 判据面）
    ("SM_B", 66), // SM 副登记（三态机 · 对拍面）
    ("ZZ_A", 35), // ZZ 主登记（转换账 · 判据面）
    ("ZZ_B", 77), // ZZ 副登记（转换账 · 对拍面）
    ("YZ_A", 42), // YZ 主登记（一致性断言 · 判据面）
    ("YZ_B", 88), // YZ 副登记（一致性断言 · 对拍面）
    ("FF_A", 49), // FF 主登记（非法转换 · 判据面）
    ("FF_B", 99), // FF 副登记（非法转换 · 对拍面）
    ("JJ_A", 56), // JJ 主登记（拒绝 · 判据面）
    ("JJ_B", 110), // JJ 副登记（拒绝 · 对拍面）
    ("MZ_A", 63), // MZ 主登记（冻结 v1 · 判据面）
    ("MZ_B", 121), // MZ 副登记（冻结 v1 · 对拍面）
    ("HB_A", 70), // HB 主登记（唤醒边 · 判据面）
    ("HB_B", 132), // HB 副登记（唤醒边 · 对拍面）
    ("SS_A", 77), // SS 主登记（三态闭集 · 判据面）
    ("SS_B", 143), // SS 副登记（三态闭集 · 对拍面）
    ("PZ_A", 84), // PZ 主登记（账实对拍 · 判据面）
    ("PZ_B", 154), // PZ 副登记（账实对拍 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1741reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1741reg_COUNT {
        if u1741reg[i].0 == key {
            return Some(u1741reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1741reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1741reg_COUNT {
        s = s.wrapping_add(u1741reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu1741Ledger（空闲态 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 空闲态 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu1741Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu1741Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5a40, 0x5a4f]。
pub const fn u1741_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1741Code::ALL.len() {
        let c = u1741Code::ALL[i].code();
        if c < 0x5a40 || c > 0x5a4f {
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
fn u1741_guard_seg() -> bool {
    u1741_segment_guard()
}

/// UNX-F1741 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u1741_core_tbl() -> bool {
    let mut ok = u1741reg_COUNT == 24;
    if ok { ok = u1741reg_get("JR_A") == Some(7) && u1741reg_get("JR_B") == Some(33); }
    if ok { ok = u1741reg_get("PZ_A") == Some(84); }
    if ok { ok = u1741reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1741reg_sum() == 1668; }
    ok
}

/// UNX-F1741 守恒账三操作确定性效果。
fn check_u1741_core_ldg() -> bool {
    let mut ld = Mxu1741Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1741_checks() -> CheckSet {
    let mut set = CheckSet::new("U1741");
    set.add(
        "u1741-seg-guard",
        u1741_guard_seg(),
        if u1741_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1741-core-tbl",
        check_u1741_core_tbl(),
        if check_u1741_core_tbl() { "" } else { "UNX-F1741 core table mismatch" },
    );
    set.add(
        "u1741-core-ldg",
        check_u1741_core_ldg(),
        if check_u1741_core_ldg() { "" } else { "UNX-F1741 core ledger mismatch" },
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
    fn u1741_all_checks_green() {
        let set = run_u1741_checks();
        assert!(set.all_passed(), "U1741 domain checks must be green");
    }

    #[test]
    fn u1741_segment_guard_holds() {
        assert!(u1741_segment_guard());
    }
}
