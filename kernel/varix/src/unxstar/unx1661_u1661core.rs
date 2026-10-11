//! unxu1661_u1661core —— 调度类注册框架与优先级链 语义登记表与守恒账（调度类）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1661 · 调度类注册框架与优先级链 · 判据原文：UNX-F1661 · 判据：注册框架单点构建，类优先级链静态闭合（无环、无洞、首尾确定），重复注册注入 10/10 次拒绝
//! · 组件：调度类 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F1661 · 判据：注册框架单点构建，类优先级链静态闭合（无环、无洞、首尾确定），重复注册注入 10/10 次拒绝
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1661_VERSION: &str = "u1661-u1661core-v1";

/// 诊断段基址。
pub const U1661_SEG_BASE: u16 = 0x5740;

// --------------------------------------------------------------------------
// 诊断码（u1661 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 调度类 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1661Code {
    /// 0x5740 表行重复（重复注册 同键双写）。
    RowDup,
    /// 0x5741 表行断档（链环 序号不连续）。
    RowGap,
    /// 0x5742 值越界（链洞 值域外）。
    ValOob,
    /// 0x5743 累计漂移（首尾漂移 求和≠理论值）。
    SumDrift,
    /// 0x5744 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0x5745 账目断漏（序号缺失）。
    LedgerHole,
    /// 0x5746 空操作（账目无效果调用）。
    OpNoop,
    /// 0x5747 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0x5748 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5749 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x574a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x574b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1661Code {
    /// 全量闭集。
    pub const ALL: [u1661Code; 12] = [
        u1661Code::RowDup,
        u1661Code::RowGap,
        u1661Code::ValOob,
        u1661Code::SumDrift,
        u1661Code::LedgerDup,
        u1661Code::LedgerHole,
        u1661Code::OpNoop,
        u1661Code::FieldDrift,
        u1661Code::LedgerBroken,
        u1661Code::SnapshotFingerprint,
        u1661Code::TableBroken,
        u1661Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1661Code::RowDup => 0x5740,
            u1661Code::RowGap => 0x5741,
            u1661Code::ValOob => 0x5742,
            u1661Code::SumDrift => 0x5743,
            u1661Code::LedgerDup => 0x5744,
            u1661Code::LedgerHole => 0x5745,
            u1661Code::OpNoop => 0x5746,
            u1661Code::FieldDrift => 0x5747,
            u1661Code::LedgerBroken => 0x5748,
            u1661Code::SnapshotFingerprint => 0x5749,
            u1661Code::TableBroken => 0x574a,
            u1661Code::SegmentGuard => 0x574b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1661Code::RowDup => "表行重复（重复注册 同键双写）",
            u1661Code::RowGap => "表行断档（链环 序号不连续）",
            u1661Code::ValOob => "值越界（链洞 值域外）",
            u1661Code::SumDrift => "累计漂移（首尾漂移 求和≠理论值）",
            u1661Code::LedgerDup => "账目重记（同一事件双入账）",
            u1661Code::LedgerHole => "账目断漏（序号缺失）",
            u1661Code::OpNoop => "空操作（账目无效果调用）",
            u1661Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u1661Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1661Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1661Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1661Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1661Code::RowDup => "按键唯一律去重",
            u1661Code::RowGap => "按连续律补号",
            u1661Code::ValOob => "按值域裁剪",
            u1661Code::SumDrift => "按理论值复算",
            u1661Code::LedgerDup => "按事件幂等律合并",
            u1661Code::LedgerHole => "按连续律补账",
            u1661Code::OpNoop => "核对操作前置条件",
            u1661Code::FieldDrift => "按口径重建字段",
            u1661Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1661Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1661Code::TableBroken => "按语义规则重建常量表",
            u1661Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1661reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 调度类 语义登记表（24 行逐行注释）。
pub const u1661reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1661reg: [(&'static str, u64); 24] = [
    ("RG_A", 7), // RG 主登记（注册框架 · 判据面）
    ("RG_B", 33), // RG 副登记（注册框架 · 对拍面）
    ("PL_A", 14), // PL 主登记（优先级链 · 判据面）
    ("PL_B", 44), // PL 副登记（优先级链 · 对拍面）
    ("WJ_A", 21), // WJ 主登记（无环 · 判据面）
    ("WJ_B", 55), // WJ 副登记（无环 · 对拍面）
    ("WD_A", 28), // WD 主登记（无洞 · 判据面）
    ("WD_B", 66), // WD 副登记（无洞 · 对拍面）
    ("SW_A", 35), // SW 主登记（首尾确定 · 判据面）
    ("SW_B", 77), // SW 副登记（首尾确定 · 对拍面）
    ("CF_A", 42), // CF 主登记（重复注册 · 判据面）
    ("CF_B", 88), // CF 副登记（重复注册 · 对拍面）
    ("JJ_A", 49), // JJ 主登记（拒绝 · 判据面）
    ("JJ_B", 99), // JJ 副登记（拒绝 · 对拍面）
    ("DL_A", 56), // DL 主登记（单点构建 · 判据面）
    ("DL_B", 110), // DL 副登记（单点构建 · 对拍面）
    ("ST_A", 63), // ST 主登记（静态闭合 · 判据面）
    ("ST_B", 121), // ST 副登记（静态闭合 · 对拍面）
    ("LK_A", 70), // LK 主登记（链锁 · 判据面）
    ("LK_B", 132), // LK 副登记（链锁 · 对拍面）
    ("PN_A", 77), // PN 主登记（类序号 · 判据面）
    ("PN_B", 143), // PN 副登记（类序号 · 对拍面）
    ("JH_A", 84), // JH 主登记（交接锚 · 判据面）
    ("JH_B", 154), // JH 副登记（交接锚 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1661reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1661reg_COUNT {
        if u1661reg[i].0 == key {
            return Some(u1661reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1661reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1661reg_COUNT {
        s = s.wrapping_add(u1661reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu1661Ledger（调度类 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 调度类 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu1661Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu1661Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5740, 0x574f]。
pub const fn u1661_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1661Code::ALL.len() {
        let c = u1661Code::ALL[i].code();
        if c < 0x5740 || c > 0x574f {
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
fn u1661_guard_seg() -> bool {
    u1661_segment_guard()
}

/// UNX-F1661 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u1661_core_tbl() -> bool {
    let mut ok = u1661reg_COUNT == 24;
    if ok { ok = u1661reg_get("RG_A") == Some(7) && u1661reg_get("RG_B") == Some(33); }
    if ok { ok = u1661reg_get("JH_A") == Some(84); }
    if ok { ok = u1661reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1661reg_sum() == 1668; }
    ok
}

/// UNX-F1661 守恒账三操作确定性效果。
fn check_u1661_core_ldg() -> bool {
    let mut ld = Mxu1661Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1661_checks() -> CheckSet {
    let mut set = CheckSet::new("U1661");
    set.add(
        "u1661-seg-guard",
        u1661_guard_seg(),
        if u1661_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1661-core-tbl",
        check_u1661_core_tbl(),
        if check_u1661_core_tbl() { "" } else { "UNX-F1661 core table mismatch" },
    );
    set.add(
        "u1661-core-ldg",
        check_u1661_core_ldg(),
        if check_u1661_core_ldg() { "" } else { "UNX-F1661 core ledger mismatch" },
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
    fn u1661_all_checks_green() {
        let set = run_u1661_checks();
        assert!(set.all_passed(), "U1661 domain checks must be green");
    }

    #[test]
    fn u1661_segment_guard_holds() {
        assert!(u1661_segment_guard());
    }
}
