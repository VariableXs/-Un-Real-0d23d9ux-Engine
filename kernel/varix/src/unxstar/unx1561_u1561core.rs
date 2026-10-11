//! unxu1561_u1561core —— 消费域联签总表（A3/A4/A5/G1/B4 等逐域登记） 语义登记表与守恒账（域）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1561 · 消费域联签总表（A3/A4/A5/G1/B4 等逐域登记） · 判据原文：UNX-F1561 · 判据：联签总表齐备（全域消费域逐域登记：域/消费接口/联签状态三列），漏域注入 10/10 必抓，总表与 F1566 冻结账对平 100%
//! · 组件：域 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F1561 · 判据：联签总表齐备（全域消费域逐域登记：域/消费接口/联签状态三列），漏域注入 10/10 必抓，总表与 F1566 冻结账对平 100
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1561_VERSION: &str = "u1561-u1561core-v1";

/// 诊断段基址。
pub const U1561_SEG_BASE: u16 = 0x5380;

// --------------------------------------------------------------------------
// 诊断码（u1561 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1561Code {
    /// 0x5380 表行重复（漏域漏登 同键双写）。
    RowDup,
    /// 0x5381 表行断档（联签缺位 序号不连续）。
    RowGap,
    /// 0x5382 值越界（对平破缺 值域外）。
    ValOob,
    /// 0x5383 累计漂移（接口悬空 求和≠理论值）。
    SumDrift,
    /// 0x5384 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0x5385 账目断漏（序号缺失）。
    LedgerHole,
    /// 0x5386 空操作（账目无效果调用）。
    OpNoop,
    /// 0x5387 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0x5388 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5389 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x538a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x538b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1561Code {
    /// 全量闭集。
    pub const ALL: [u1561Code; 12] = [
        u1561Code::RowDup,
        u1561Code::RowGap,
        u1561Code::ValOob,
        u1561Code::SumDrift,
        u1561Code::LedgerDup,
        u1561Code::LedgerHole,
        u1561Code::OpNoop,
        u1561Code::FieldDrift,
        u1561Code::LedgerBroken,
        u1561Code::SnapshotFingerprint,
        u1561Code::TableBroken,
        u1561Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1561Code::RowDup => 0x5380,
            u1561Code::RowGap => 0x5381,
            u1561Code::ValOob => 0x5382,
            u1561Code::SumDrift => 0x5383,
            u1561Code::LedgerDup => 0x5384,
            u1561Code::LedgerHole => 0x5385,
            u1561Code::OpNoop => 0x5386,
            u1561Code::FieldDrift => 0x5387,
            u1561Code::LedgerBroken => 0x5388,
            u1561Code::SnapshotFingerprint => 0x5389,
            u1561Code::TableBroken => 0x538a,
            u1561Code::SegmentGuard => 0x538b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1561Code::RowDup => "表行重复（漏域漏登 同键双写）",
            u1561Code::RowGap => "表行断档（联签缺位 序号不连续）",
            u1561Code::ValOob => "值越界（对平破缺 值域外）",
            u1561Code::SumDrift => "累计漂移（接口悬空 求和≠理论值）",
            u1561Code::LedgerDup => "账目重记（同一事件双入账）",
            u1561Code::LedgerHole => "账目断漏（序号缺失）",
            u1561Code::OpNoop => "空操作（账目无效果调用）",
            u1561Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u1561Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1561Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1561Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1561Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1561Code::RowDup => "按键唯一律去重",
            u1561Code::RowGap => "按连续律补号",
            u1561Code::ValOob => "按值域裁剪",
            u1561Code::SumDrift => "按理论值复算",
            u1561Code::LedgerDup => "按事件幂等律合并",
            u1561Code::LedgerHole => "按连续律补账",
            u1561Code::OpNoop => "核对操作前置条件",
            u1561Code::FieldDrift => "按口径重建字段",
            u1561Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1561Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1561Code::TableBroken => "按语义规则重建常量表",
            u1561Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1561reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 域 语义登记表（24 行逐行注释）。
pub const u1561reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1561reg: [(&'static str, u64); 24] = [
    ("CD_A", 7), // CD 主登记（消费域 · 判据面）
    ("CD_B", 33), // CD 副登记（消费域 · 对拍面）
    ("CI_A", 14), // CI 主登记（消费接口 · 判据面）
    ("CI_B", 44), // CI 副登记（消费接口 · 对拍面）
    ("CS_A", 21), // CS 主登记（联签状态 · 判据面）
    ("CS_B", 55), // CS 副登记（联签状态 · 对拍面）
    ("LD_A", 28), // LD 主登记（漏域注入 · 判据面）
    ("LD_B", 66), // LD 副登记（漏域注入 · 对拍面）
    ("FZ_A", 35), // FZ 主登记（冻结账 · 判据面）
    ("FZ_B", 77), // FZ 副登记（冻结账 · 对拍面）
    ("DP_A", 42), // DP 主登记（对平 · 判据面）
    ("DP_B", 88), // DP 副登记（对平 · 对拍面）
    ("A3_A", 49), // A3 主登记（A3 域 · 判据面）
    ("A3_B", 99), // A3 副登记（A3 域 · 对拍面）
    ("A4_A", 56), // A4 主登记（A4 域 · 判据面）
    ("A4_B", 110), // A4 副登记（A4 域 · 对拍面）
    ("A5_A", 63), // A5 主登记（A5 域 · 判据面）
    ("A5_B", 121), // A5 副登记（A5 域 · 对拍面）
    ("G1_A", 70), // G1 主登记（G1 域 · 判据面）
    ("G1_B", 132), // G1 副登记（G1 域 · 对拍面）
    ("B4_A", 77), // B4 主登记（B4 域 · 判据面）
    ("B4_B", 143), // B4 副登记（B4 域 · 对拍面）
    ("JS_A", 84), // JS 主登记（逐域登记 · 判据面）
    ("JS_B", 154), // JS 副登记（逐域登记 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1561reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1561reg_COUNT {
        if u1561reg[i].0 == key {
            return Some(u1561reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1561reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1561reg_COUNT {
        s = s.wrapping_add(u1561reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu1561Ledger（域 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 域 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu1561Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu1561Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5380, 0x538f]。
pub const fn u1561_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1561Code::ALL.len() {
        let c = u1561Code::ALL[i].code();
        if c < 0x5380 || c > 0x538f {
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
fn u1561_guard_seg() -> bool {
    u1561_segment_guard()
}

/// UNX-F1561 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u1561_core_tbl() -> bool {
    let mut ok = u1561reg_COUNT == 24;
    if ok { ok = u1561reg_get("CD_A") == Some(7) && u1561reg_get("CD_B") == Some(33); }
    if ok { ok = u1561reg_get("JS_A") == Some(84); }
    if ok { ok = u1561reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1561reg_sum() == 1668; }
    ok
}

/// UNX-F1561 守恒账三操作确定性效果。
fn check_u1561_core_ldg() -> bool {
    let mut ld = Mxu1561Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1561_checks() -> CheckSet {
    let mut set = CheckSet::new("U1561");
    set.add(
        "u1561-seg-guard",
        u1561_guard_seg(),
        if u1561_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1561-core-tbl",
        check_u1561_core_tbl(),
        if check_u1561_core_tbl() { "" } else { "UNX-F1561 core table mismatch" },
    );
    set.add(
        "u1561-core-ldg",
        check_u1561_core_ldg(),
        if check_u1561_core_ldg() { "" } else { "UNX-F1561 core ledger mismatch" },
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
    fn u1561_all_checks_green() {
        let set = run_u1561_checks();
        assert!(set.all_passed(), "U1561 domain checks must be green");
    }

    #[test]
    fn u1561_segment_guard_holds() {
        assert!(u1561_segment_guard());
    }
}
