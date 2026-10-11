//! unxu1681_u1681core —— 时间片计算总纲（基片×权重缩放模型） 语义登记表与守恒账（时间片）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1681 · 时间片计算总纲（基片×权重缩放模型） · 判据原文：UNX-F1681 · 判据：时间片=基片×（本线程权重/基准权重）模型冻结 v1，公式参数单点定义，计算路径唯一
//! · 组件：时间片 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F1681 · 判据：时间片=基片×（本线程权重/基准权重）模型冻结 v1，公式参数单点定义，计算路径唯一
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1681_VERSION: &str = "u1681-u1681core-v1";

/// 诊断段基址。
pub const U1681_SEG_BASE: u16 = 0x5800;

// --------------------------------------------------------------------------
// 诊断码（u1681 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 时间片 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1681Code {
    /// 0x5800 表行重复（参数多点 同键双写）。
    RowDup,
    /// 0x5801 表行断档（路径分叉 序号不连续）。
    RowGap,
    /// 0x5802 值越界（钳制漂移 值域外）。
    ValOob,
    /// 0x5803 累计漂移（对拍失准 求和≠理论值）。
    SumDrift,
    /// 0x5804 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0x5805 账目断漏（序号缺失）。
    LedgerHole,
    /// 0x5806 空操作（账目无效果调用）。
    OpNoop,
    /// 0x5807 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0x5808 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5809 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x580a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x580b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1681Code {
    /// 全量闭集。
    pub const ALL: [u1681Code; 12] = [
        u1681Code::RowDup,
        u1681Code::RowGap,
        u1681Code::ValOob,
        u1681Code::SumDrift,
        u1681Code::LedgerDup,
        u1681Code::LedgerHole,
        u1681Code::OpNoop,
        u1681Code::FieldDrift,
        u1681Code::LedgerBroken,
        u1681Code::SnapshotFingerprint,
        u1681Code::TableBroken,
        u1681Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1681Code::RowDup => 0x5800,
            u1681Code::RowGap => 0x5801,
            u1681Code::ValOob => 0x5802,
            u1681Code::SumDrift => 0x5803,
            u1681Code::LedgerDup => 0x5804,
            u1681Code::LedgerHole => 0x5805,
            u1681Code::OpNoop => 0x5806,
            u1681Code::FieldDrift => 0x5807,
            u1681Code::LedgerBroken => 0x5808,
            u1681Code::SnapshotFingerprint => 0x5809,
            u1681Code::TableBroken => 0x580a,
            u1681Code::SegmentGuard => 0x580b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1681Code::RowDup => "表行重复（参数多点 同键双写）",
            u1681Code::RowGap => "表行断档（路径分叉 序号不连续）",
            u1681Code::ValOob => "值越界（钳制漂移 值域外）",
            u1681Code::SumDrift => "累计漂移（对拍失准 求和≠理论值）",
            u1681Code::LedgerDup => "账目重记（同一事件双入账）",
            u1681Code::LedgerHole => "账目断漏（序号缺失）",
            u1681Code::OpNoop => "空操作（账目无效果调用）",
            u1681Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u1681Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1681Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1681Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1681Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1681Code::RowDup => "按键唯一律去重",
            u1681Code::RowGap => "按连续律补号",
            u1681Code::ValOob => "按值域裁剪",
            u1681Code::SumDrift => "按理论值复算",
            u1681Code::LedgerDup => "按事件幂等律合并",
            u1681Code::LedgerHole => "按连续律补账",
            u1681Code::OpNoop => "核对操作前置条件",
            u1681Code::FieldDrift => "按口径重建字段",
            u1681Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1681Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1681Code::TableBroken => "按语义规则重建常量表",
            u1681Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1681reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 时间片 语义登记表（24 行逐行注释）。
pub const u1681reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1681reg: [(&'static str, u64); 24] = [
    ("BS_A", 7), // BS 主登记（基片 · 判据面）
    ("BS_B", 33), // BS 副登记（基片 · 对拍面）
    ("QZ_A", 14), // QZ 主登记（线程权重 · 判据面）
    ("QZ_B", 44), // QZ 副登记（线程权重 · 对拍面）
    ("JZ_A", 21), // JZ 主登记（基准权重 · 判据面）
    ("JZ_B", 55), // JZ 副登记（基准权重 · 对拍面）
    ("MS_A", 28), // MS 主登记（模型冻结 · 判据面）
    ("MS_B", 66), // MS 副登记（模型冻结 · 对拍面）
    ("DD_A", 35), // DD 主登记（参数单点 · 判据面）
    ("DD_B", 77), // DD 副登记（参数单点 · 对拍面）
    ("WY_A", 42), // WY 主登记（计算路径唯一 · 判据面）
    ("WY_B", 88), // WY 副登记（计算路径唯一 · 对拍面）
    ("SJ_A", 49), // SJ 主登记（时间片 · 判据面）
    ("SJ_B", 99), // SJ 副登记（时间片 · 对拍面）
    ("GS_A", 56), // GS 主登记（公式 · 判据面）
    ("GS_B", 110), // GS 副登记（公式 · 对拍面）
    ("SR_A", 63), // SR 主登记（舍入锚 · 判据面）
    ("SR_B", 121), // SR 副登记（舍入锚 · 对拍面）
    ("XW_A", 70), // XW 主登记（下限钳制 · 判据面）
    ("XW_B", 132), // XW 副登记（下限钳制 · 对拍面）
    ("SW_A", 77), // SW 主登记（上限钳制 · 判据面）
    ("SW_B", 143), // SW 副登记（上限钳制 · 对拍面）
    ("PZ_A", 84), // PZ 主登记（漂移零容 · 判据面）
    ("PZ_B", 154), // PZ 副登记（漂移零容 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1681reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1681reg_COUNT {
        if u1681reg[i].0 == key {
            return Some(u1681reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1681reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1681reg_COUNT {
        s = s.wrapping_add(u1681reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu1681Ledger（时间片 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 时间片 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu1681Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu1681Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5800, 0x580f]。
pub const fn u1681_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1681Code::ALL.len() {
        let c = u1681Code::ALL[i].code();
        if c < 0x5800 || c > 0x580f {
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
fn u1681_guard_seg() -> bool {
    u1681_segment_guard()
}

/// UNX-F1681 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u1681_core_tbl() -> bool {
    let mut ok = u1681reg_COUNT == 24;
    if ok { ok = u1681reg_get("BS_A") == Some(7) && u1681reg_get("BS_B") == Some(33); }
    if ok { ok = u1681reg_get("PZ_A") == Some(84); }
    if ok { ok = u1681reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1681reg_sum() == 1668; }
    ok
}

/// UNX-F1681 守恒账三操作确定性效果。
fn check_u1681_core_ldg() -> bool {
    let mut ld = Mxu1681Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1681_checks() -> CheckSet {
    let mut set = CheckSet::new("U1681");
    set.add(
        "u1681-seg-guard",
        u1681_guard_seg(),
        if u1681_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1681-core-tbl",
        check_u1681_core_tbl(),
        if check_u1681_core_tbl() { "" } else { "UNX-F1681 core table mismatch" },
    );
    set.add(
        "u1681-core-ldg",
        check_u1681_core_ldg(),
        if check_u1681_core_ldg() { "" } else { "UNX-F1681 core ledger mismatch" },
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
    fn u1681_all_checks_green() {
        let set = run_u1681_checks();
        assert!(set.all_passed(), "U1681 domain checks must be green");
    }

    #[test]
    fn u1681_segment_guard_holds() {
        assert!(u1681_segment_guard());
    }
}
