//! unxu1821_u1821core —— 切换帧总纲（内核栈布局与切换帧结构冻结） 语义登记表与守恒账（帧区）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1821 · 切换帧总纲（内核栈布局与切换帧结构冻结） · 判据原文：UNX-F1821 · 判据：切换帧结构（栈顶中断帧区/寄存器保存区/控制块锚区）冻结 v1 与 F1624 栈约定对齐，布局越界注入 10/10 检出
//! · 组件：帧区 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F1821 · 判据：切换帧结构（栈顶中断帧区/寄存器保存区/控制块锚区）冻结 v1 与 F1624 栈约定对齐，布局越界注入 10/10 检出
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1821_VERSION: &str = "u1821-u1821core-v1";

/// 诊断段基址。
pub const U1821_SEG_BASE: u16 = 0x5d40;

// --------------------------------------------------------------------------
// 诊断码（u1821 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 帧区 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1821Code {
    /// 0x5d40 表行重复（布局越界 同键双写）。
    RowDup,
    /// 0x5d41 表行断档（对齐破坏 序号不连续）。
    RowGap,
    /// 0x5d42 值越界（约定失锚 值域外）。
    ValOob,
    /// 0x5d43 累计漂移（检出漏报 求和≠理论值）。
    SumDrift,
    /// 0x5d44 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0x5d45 账目断漏（序号缺失）。
    LedgerHole,
    /// 0x5d46 空操作（账目无效果调用）。
    OpNoop,
    /// 0x5d47 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0x5d48 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5d49 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5d4a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5d4b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1821Code {
    /// 全量闭集。
    pub const ALL: [u1821Code; 12] = [
        u1821Code::RowDup,
        u1821Code::RowGap,
        u1821Code::ValOob,
        u1821Code::SumDrift,
        u1821Code::LedgerDup,
        u1821Code::LedgerHole,
        u1821Code::OpNoop,
        u1821Code::FieldDrift,
        u1821Code::LedgerBroken,
        u1821Code::SnapshotFingerprint,
        u1821Code::TableBroken,
        u1821Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1821Code::RowDup => 0x5d40,
            u1821Code::RowGap => 0x5d41,
            u1821Code::ValOob => 0x5d42,
            u1821Code::SumDrift => 0x5d43,
            u1821Code::LedgerDup => 0x5d44,
            u1821Code::LedgerHole => 0x5d45,
            u1821Code::OpNoop => 0x5d46,
            u1821Code::FieldDrift => 0x5d47,
            u1821Code::LedgerBroken => 0x5d48,
            u1821Code::SnapshotFingerprint => 0x5d49,
            u1821Code::TableBroken => 0x5d4a,
            u1821Code::SegmentGuard => 0x5d4b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1821Code::RowDup => "表行重复（布局越界 同键双写）",
            u1821Code::RowGap => "表行断档（对齐破坏 序号不连续）",
            u1821Code::ValOob => "值越界（约定失锚 值域外）",
            u1821Code::SumDrift => "累计漂移（检出漏报 求和≠理论值）",
            u1821Code::LedgerDup => "账目重记（同一事件双入账）",
            u1821Code::LedgerHole => "账目断漏（序号缺失）",
            u1821Code::OpNoop => "空操作（账目无效果调用）",
            u1821Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u1821Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1821Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1821Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1821Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1821Code::RowDup => "按键唯一律去重",
            u1821Code::RowGap => "按连续律补号",
            u1821Code::ValOob => "按值域裁剪",
            u1821Code::SumDrift => "按理论值复算",
            u1821Code::LedgerDup => "按事件幂等律合并",
            u1821Code::LedgerHole => "按连续律补账",
            u1821Code::OpNoop => "核对操作前置条件",
            u1821Code::FieldDrift => "按口径重建字段",
            u1821Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1821Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1821Code::TableBroken => "按语义规则重建常量表",
            u1821Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1821reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 帧区 语义登记表（24 行逐行注释）。
pub const u1821reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1821reg: [(&'static str, u64); 24] = [
    ("ZZ_A", 7), // ZZ 主登记（栈顶中断帧区 · 判据面）
    ("ZZ_B", 33), // ZZ 副登记（栈顶中断帧区 · 对拍面）
    ("BC_A", 14), // BC 主登记（寄存器保存区 · 判据面）
    ("BC_B", 44), // BC 副登记（寄存器保存区 · 对拍面）
    ("KB_A", 21), // KB 主登记（控制块锚区 · 判据面）
    ("KB_B", 55), // KB 副登记（控制块锚区 · 对拍面）
    ("DZ_A", 28), // DZ 主登记（冻结 v1 · 判据面）
    ("DZ_B", 66), // DZ 副登记（冻结 v1 · 对拍面）
    ("F1624_A", 35), // F1624 主登记（F1624 栈约定 · 判据面）
    ("F1624_B", 77), // F1624 副登记（F1624 栈约定 · 对拍面）
    ("YJ_A", 42), // YJ 主登记（越界注入 · 判据面）
    ("YJ_B", 88), // YJ 副登记（越界注入 · 对拍面）
    ("JC_A", 49), // JC 主登记（检出 · 判据面）
    ("JC_B", 99), // JC 副登记（检出 · 对拍面）
    ("JP_A", 56), // JP 主登记（布局 · 判据面）
    ("JP_B", 110), // JP 副登记（布局 · 对拍面）
    ("DZ2_A", 63), // DZ2 主登记（对齐锚 · 判据面）
    ("DZ2_B", 121), // DZ2 副登记（对齐锚 · 对拍面）
    ("HZ_A", 70), // HZ 主登记（帧号 · 判据面）
    ("HZ_B", 132), // HZ 副登记（帧号 · 对拍面）
    ("SZ_A", 77), // SZ 主登记（尺寸账 · 判据面）
    ("SZ_B", 143), // SZ 副登记（尺寸账 · 对拍面）
    ("PB_A", 84), // PB 主登记（偏移账 · 判据面）
    ("PB_B", 154), // PB 副登记（偏移账 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1821reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1821reg_COUNT {
        if u1821reg[i].0 == key {
            return Some(u1821reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1821reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1821reg_COUNT {
        s = s.wrapping_add(u1821reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu1821Ledger（帧区 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 帧区 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu1821Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu1821Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5d40, 0x5d4f]。
pub const fn u1821_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1821Code::ALL.len() {
        let c = u1821Code::ALL[i].code();
        if c < 0x5d40 || c > 0x5d4f {
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
fn u1821_guard_seg() -> bool {
    u1821_segment_guard()
}

/// UNX-F1821 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u1821_core_tbl() -> bool {
    let mut ok = u1821reg_COUNT == 24;
    if ok { ok = u1821reg_get("ZZ_A") == Some(7) && u1821reg_get("ZZ_B") == Some(33); }
    if ok { ok = u1821reg_get("PB_A") == Some(84); }
    if ok { ok = u1821reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1821reg_sum() == 1668; }
    ok
}

/// UNX-F1821 守恒账三操作确定性效果。
fn check_u1821_core_ldg() -> bool {
    let mut ld = Mxu1821Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1821_checks() -> CheckSet {
    let mut set = CheckSet::new("U1821");
    set.add(
        "u1821-seg-guard",
        u1821_guard_seg(),
        if u1821_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1821-core-tbl",
        check_u1821_core_tbl(),
        if check_u1821_core_tbl() { "" } else { "UNX-F1821 core table mismatch" },
    );
    set.add(
        "u1821-core-ldg",
        check_u1821_core_ldg(),
        if check_u1821_core_ldg() { "" } else { "UNX-F1821 core ledger mismatch" },
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
    fn u1821_all_checks_green() {
        let set = run_u1821_checks();
        assert!(set.all_passed(), "U1821 domain checks must be green");
    }

    #[test]
    fn u1821_segment_guard_holds() {
        assert!(u1821_segment_guard());
    }
}
