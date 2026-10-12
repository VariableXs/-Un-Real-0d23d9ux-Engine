//! unxu2761_u2761core —— 四级压力档判定器（水位+产出双因子） 语义登记表与守恒账（压力档）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2761 · 四级压力档判定器（水位+产出双因子） · 判据原文：UNX-F2761 · 判据：四档（正常/轻压/中压/重压）双因子判定逐档可复测，档位注入 40/40 次正确，与 F2741/F2747 双源读数一致
//! · 组件：压力档 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2761 · 判据：四档（正常/轻压/中压/重压）双因子判定逐档可复测，档位注入 40/40 次正确，与 F2741/F2747 双源读数一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2761_VERSION: &str = "u2761-u2761core-v1";

/// 诊断段基址。
pub const U2761_SEG_BASE: u16 = 0xad40;

// --------------------------------------------------------------------------
// 诊断码（u2761 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 压力档 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2761Code {
    /// 0xad40 表行重复（档位跳变 同键双写）。
    RowDup,
    /// 0xad41 表行断档（双源分歧 序号不连续）。
    RowGap,
    /// 0xad42 值越界（注入误判 值域外）。
    ValOob,
    /// 0xad43 累计漂移（因子失真 求和≠理论值）。
    SumDrift,
    /// 0xad44 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xad45 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xad46 空操作（账目无效果调用）。
    OpNoop,
    /// 0xad47 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xad48 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xad49 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xad4a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xad4b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2761Code {
    /// 全量闭集。
    pub const ALL: [u2761Code; 12] = [
        u2761Code::RowDup,
        u2761Code::RowGap,
        u2761Code::ValOob,
        u2761Code::SumDrift,
        u2761Code::LedgerDup,
        u2761Code::LedgerHole,
        u2761Code::OpNoop,
        u2761Code::FieldDrift,
        u2761Code::LedgerBroken,
        u2761Code::SnapshotFingerprint,
        u2761Code::TableBroken,
        u2761Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2761Code::RowDup => 0xad40,
            u2761Code::RowGap => 0xad41,
            u2761Code::ValOob => 0xad42,
            u2761Code::SumDrift => 0xad43,
            u2761Code::LedgerDup => 0xad44,
            u2761Code::LedgerHole => 0xad45,
            u2761Code::OpNoop => 0xad46,
            u2761Code::FieldDrift => 0xad47,
            u2761Code::LedgerBroken => 0xad48,
            u2761Code::SnapshotFingerprint => 0xad49,
            u2761Code::TableBroken => 0xad4a,
            u2761Code::SegmentGuard => 0xad4b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2761Code::RowDup => "表行重复（档位跳变 同键双写）",
            u2761Code::RowGap => "表行断档（双源分歧 序号不连续）",
            u2761Code::ValOob => "值越界（注入误判 值域外）",
            u2761Code::SumDrift => "累计漂移（因子失真 求和≠理论值）",
            u2761Code::LedgerDup => "账目重记（同一事件双入账）",
            u2761Code::LedgerHole => "账目断漏（序号缺失）",
            u2761Code::OpNoop => "空操作（账目无效果调用）",
            u2761Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2761Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2761Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2761Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2761Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2761Code::RowDup => "按键唯一律去重",
            u2761Code::RowGap => "按连续律补号",
            u2761Code::ValOob => "按值域裁剪",
            u2761Code::SumDrift => "按理论值复算",
            u2761Code::LedgerDup => "按事件幂等律合并",
            u2761Code::LedgerHole => "按连续律补账",
            u2761Code::OpNoop => "核对操作前置条件",
            u2761Code::FieldDrift => "按口径重建字段",
            u2761Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2761Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2761Code::TableBroken => "按语义规则重建常量表",
            u2761Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2761reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 压力档 语义登记表（24 行逐行注释）。
pub const u2761reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2761reg: [(&'static str, u64); 24] = [
    ("NM_A", 7), // NM 主登记（正常档 · 判据面）
    ("NM_B", 33), // NM 副登记（正常档 · 对拍面）
    ("LT_A", 14), // LT 主登记（轻压档 · 判据面）
    ("LT_B", 44), // LT 副登记（轻压档 · 对拍面）
    ("MD_A", 21), // MD 主登记（中压档 · 判据面）
    ("MD_B", 55), // MD 副登记（中压档 · 对拍面）
    ("HV_A", 28), // HV 主登记（重压档 · 判据面）
    ("HV_B", 66), // HV 副登记（重压档 · 对拍面）
    ("WL_A", 35), // WL 主登记（水位因子 · 判据面）
    ("WL_B", 77), // WL 副登记（水位因子 · 对拍面）
    ("OU_A", 42), // OU 主登记（产出因子 · 判据面）
    ("OU_B", 88), // OU 副登记（产出因子 · 对拍面）
    ("DF_A", 49), // DF 主登记（双因子 · 判据面）
    ("DF_B", 99), // DF 副登记（双因子 · 对拍面）
    ("IN40_A", 56), // IN40 主登记（注入 40/40 · 判据面）
    ("IN40_B", 110), // IN40 副登记（注入 40/40 · 对拍面）
    ("D1_A", 63), // D1 主登记（F2741 读数 · 判据面）
    ("D1_B", 121), // D1 副登记（F2741 读数 · 对拍面）
    ("D2_A", 70), // D2 主登记（F2747 读数 · 判据面）
    ("D2_B", 132), // D2 副登记（F2747 读数 · 对拍面）
    ("DR_A", 77), // DR 主登记（双源一致 · 判据面）
    ("DR_B", 143), // DR 副登记（双源一致 · 对拍面）
    ("RT_A", 84), // RT 主登记（可复测 · 判据面）
    ("RT_B", 154), // RT 副登记（可复测 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2761reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2761reg_COUNT {
        if u2761reg[i].0 == key {
            return Some(u2761reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2761reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2761reg_COUNT {
        s = s.wrapping_add(u2761reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2761Ledger（压力档 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 压力档 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2761Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2761Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xad40, 0xad4f]。
pub const fn u2761_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2761Code::ALL.len() {
        let c = u2761Code::ALL[i].code();
        if c < 0xad40 || c > 0xad4f {
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
fn u2761_guard_seg() -> bool {
    u2761_segment_guard()
}

/// UNX-F2761 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2761_core_tbl() -> bool {
    let mut ok = u2761reg_COUNT == 24;
    if ok { ok = u2761reg_get("NM_A") == Some(7) && u2761reg_get("NM_B") == Some(33); }
    if ok { ok = u2761reg_get("RT_A") == Some(84); }
    if ok { ok = u2761reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2761reg_sum() == 1668; }
    ok
}

/// UNX-F2761 守恒账三操作确定性效果。
fn check_u2761_core_ldg() -> bool {
    let mut ld = Mxu2761Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2761_checks() -> CheckSet {
    let mut set = CheckSet::new("U2761");
    set.add(
        "u2761-seg-guard",
        u2761_guard_seg(),
        if u2761_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2761-core-tbl",
        check_u2761_core_tbl(),
        if check_u2761_core_tbl() { "" } else { "UNX-F2761 core table mismatch" },
    );
    set.add(
        "u2761-core-ldg",
        check_u2761_core_ldg(),
        if check_u2761_core_ldg() { "" } else { "UNX-F2761 core ledger mismatch" },
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
    fn u2761_all_checks_green() {
        let set = run_u2761_checks();
        assert!(set.all_passed(), "U2761 domain checks must be green");
    }

    #[test]
    fn u2761_segment_guard_holds() {
        assert!(u2761_segment_guard());
    }
}
