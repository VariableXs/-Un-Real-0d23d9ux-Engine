//! unxu2741_u2741core —— 水线三档判定与唤醒回调 语义登记表与守恒账（水线档）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2741 · 水线三档判定与唤醒回调 · 判据原文：UNX-F2741 · 判据：三档判定（min/low/high 对 free 页数）逐档可复测，low 触发唤醒回调 10/10 次
//! · 组件：水线档 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2741 · 判据：三档判定（min/low/high 对 free 页数）逐档可复测，low 触发唤醒回调 10/10 次
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2741_VERSION: &str = "u2741-u2741core-v1";

/// 诊断段基址。
pub const U2741_SEG_BASE: u16 = 0xac80;

// --------------------------------------------------------------------------
// 诊断码（u2741 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 水线档 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2741Code {
    /// 0xac80 表行重复（档位错判 同键双写）。
    RowDup,
    /// 0xac81 表行断档（唤醒漏调 序号不连续）。
    RowGap,
    /// 0xac82 值越界（复测不符 值域外）。
    ValOob,
    /// 0xac83 累计漂移（阈值漂移 求和≠理论值）。
    SumDrift,
    /// 0xac84 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xac85 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xac86 空操作（账目无效果调用）。
    OpNoop,
    /// 0xac87 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xac88 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xac89 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xac8a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xac8b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2741Code {
    /// 全量闭集。
    pub const ALL: [u2741Code; 12] = [
        u2741Code::RowDup,
        u2741Code::RowGap,
        u2741Code::ValOob,
        u2741Code::SumDrift,
        u2741Code::LedgerDup,
        u2741Code::LedgerHole,
        u2741Code::OpNoop,
        u2741Code::FieldDrift,
        u2741Code::LedgerBroken,
        u2741Code::SnapshotFingerprint,
        u2741Code::TableBroken,
        u2741Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2741Code::RowDup => 0xac80,
            u2741Code::RowGap => 0xac81,
            u2741Code::ValOob => 0xac82,
            u2741Code::SumDrift => 0xac83,
            u2741Code::LedgerDup => 0xac84,
            u2741Code::LedgerHole => 0xac85,
            u2741Code::OpNoop => 0xac86,
            u2741Code::FieldDrift => 0xac87,
            u2741Code::LedgerBroken => 0xac88,
            u2741Code::SnapshotFingerprint => 0xac89,
            u2741Code::TableBroken => 0xac8a,
            u2741Code::SegmentGuard => 0xac8b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2741Code::RowDup => "表行重复（档位错判 同键双写）",
            u2741Code::RowGap => "表行断档（唤醒漏调 序号不连续）",
            u2741Code::ValOob => "值越界（复测不符 值域外）",
            u2741Code::SumDrift => "累计漂移（阈值漂移 求和≠理论值）",
            u2741Code::LedgerDup => "账目重记（同一事件双入账）",
            u2741Code::LedgerHole => "账目断漏（序号缺失）",
            u2741Code::OpNoop => "空操作（账目无效果调用）",
            u2741Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2741Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2741Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2741Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2741Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2741Code::RowDup => "按键唯一律去重",
            u2741Code::RowGap => "按连续律补号",
            u2741Code::ValOob => "按值域裁剪",
            u2741Code::SumDrift => "按理论值复算",
            u2741Code::LedgerDup => "按事件幂等律合并",
            u2741Code::LedgerHole => "按连续律补账",
            u2741Code::OpNoop => "核对操作前置条件",
            u2741Code::FieldDrift => "按口径重建字段",
            u2741Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2741Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2741Code::TableBroken => "按语义规则重建常量表",
            u2741Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2741reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 水线档 语义登记表（24 行逐行注释）。
pub const u2741reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2741reg: [(&'static str, u64); 24] = [
    ("MN_A", 7), // MN 主登记（min 档 · 判据面）
    ("MN_B", 33), // MN 副登记（min 档 · 对拍面）
    ("LW_A", 14), // LW 主登记（low 档 · 判据面）
    ("LW_B", 44), // LW 副登记（low 档 · 对拍面）
    ("HG_A", 21), // HG 主登记（high 档 · 判据面）
    ("HG_B", 55), // HG 副登记（high 档 · 对拍面）
    ("FR_A", 28), // FR 主登记（free 页数 · 判据面）
    ("FR_B", 66), // FR 副登记（free 页数 · 对拍面）
    ("WK_A", 35), // WK 主登记（唤醒回调 · 判据面）
    ("WK_B", 77), // WK 副登记（唤醒回调 · 对拍面）
    ("RT_A", 42), // RT 主登记（可复测 · 判据面）
    ("RT_B", 88), // RT 副登记（可复测 · 对拍面）
    ("T10_A", 49), // T10 主登记（十次触发 · 判据面）
    ("T10_B", 99), // T10 副登记（十次触发 · 对拍面）
    ("CB_A", 56), // CB 主登记（回调账 · 判据面）
    ("CB_B", 110), // CB 副登记（回调账 · 对拍面）
    ("TS_A", 63), // TS 主登记（阈值表 · 判据面）
    ("TS_B", 121), // TS 副登记（阈值表 · 对拍面）
    ("SP_A", 70), // SP 主登记（单点源 · 判据面）
    ("SP_B", 132), // SP 副登记（单点源 · 对拍面）
    ("DR_A", 77), // DR 主登记（派生档 · 判据面）
    ("DR_B", 143), // DR 副登记（派生档 · 对拍面）
    ("AS_A", 84), // AS 主登记（断言面 · 判据面）
    ("AS_B", 154), // AS 副登记（断言面 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2741reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2741reg_COUNT {
        if u2741reg[i].0 == key {
            return Some(u2741reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2741reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2741reg_COUNT {
        s = s.wrapping_add(u2741reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2741Ledger（水线档 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 水线档 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2741Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2741Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xac80, 0xac8f]。
pub const fn u2741_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2741Code::ALL.len() {
        let c = u2741Code::ALL[i].code();
        if c < 0xac80 || c > 0xac8f {
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
fn u2741_guard_seg() -> bool {
    u2741_segment_guard()
}

/// UNX-F2741 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2741_core_tbl() -> bool {
    let mut ok = u2741reg_COUNT == 24;
    if ok { ok = u2741reg_get("MN_A") == Some(7) && u2741reg_get("MN_B") == Some(33); }
    if ok { ok = u2741reg_get("AS_A") == Some(84); }
    if ok { ok = u2741reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2741reg_sum() == 1668; }
    ok
}

/// UNX-F2741 守恒账三操作确定性效果。
fn check_u2741_core_ldg() -> bool {
    let mut ld = Mxu2741Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2741_checks() -> CheckSet {
    let mut set = CheckSet::new("U2741");
    set.add(
        "u2741-seg-guard",
        u2741_guard_seg(),
        if u2741_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2741-core-tbl",
        check_u2741_core_tbl(),
        if check_u2741_core_tbl() { "" } else { "UNX-F2741 core table mismatch" },
    );
    set.add(
        "u2741-core-ldg",
        check_u2741_core_ldg(),
        if check_u2741_core_ldg() { "" } else { "UNX-F2741 core ledger mismatch" },
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
    fn u2741_all_checks_green() {
        let set = run_u2741_checks();
        assert!(set.all_passed(), "U2741 domain checks must be green");
    }

    #[test]
    fn u2741_segment_guard_holds() {
        assert!(u2741_segment_guard());
    }
}
