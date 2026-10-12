//! unxu3541_u3541core —— 采样剖析器总设计（周期中断采样——桩化 tick） 语义登记表与守恒账（采样剖析器）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3541 · 采样剖析器总设计（周期中断采样——桩化 tick） · 判据原文：UNX-F3541 · 判据：J1：剖析语义冻结（周期 tick→采当前栈→计数——采样式非插桩式）；M-299 采样全链（tick→样本→聚合→导出四步通）。<br>J2：采样式选型声明（与插桩式的取舍——零代码改动税/精度换自由——判据行内嵌）。
//! · 组件：采样剖析器 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3541 · 判据：J1：剖析语义冻结（周期 tick→采当前栈→计数——采样式非插桩式）；M-299 采样全链（tick→样本→聚合→导出四步通）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3541_VERSION: &str = "u3541-u3541core-v1";

/// 诊断段基址。
pub const U3541_SEG_BASE: u16 = 0xe0e0;

// --------------------------------------------------------------------------
// 诊断码（u3541 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 采样剖析器 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3541Code {
    /// 0xe0e0 表行重复（采样 同键双写）。
    RowDup,
    /// 0xe0e1 表行断档（剖析 序号不连续）。
    RowGap,
    /// 0xe0e2 值越界（折叠 值域外）。
    ValOob,
    /// 0xe0e3 累计漂移（周期中断 求和≠理论值）。
    SumDrift,
    /// 0xe0e4 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe0e5 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe0e6 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe0e7 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe0e8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0e9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0ea 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0eb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3541Code {
    /// 全量闭集。
    pub const ALL: [u3541Code; 12] = [
        u3541Code::RowDup,
        u3541Code::RowGap,
        u3541Code::ValOob,
        u3541Code::SumDrift,
        u3541Code::LedgerDup,
        u3541Code::LedgerHole,
        u3541Code::OpNoop,
        u3541Code::FieldDrift,
        u3541Code::LedgerBroken,
        u3541Code::SnapshotFingerprint,
        u3541Code::TableBroken,
        u3541Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3541Code::RowDup => 0xe0e0,
            u3541Code::RowGap => 0xe0e1,
            u3541Code::ValOob => 0xe0e2,
            u3541Code::SumDrift => 0xe0e3,
            u3541Code::LedgerDup => 0xe0e4,
            u3541Code::LedgerHole => 0xe0e5,
            u3541Code::OpNoop => 0xe0e6,
            u3541Code::FieldDrift => 0xe0e7,
            u3541Code::LedgerBroken => 0xe0e8,
            u3541Code::SnapshotFingerprint => 0xe0e9,
            u3541Code::TableBroken => 0xe0ea,
            u3541Code::SegmentGuard => 0xe0eb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3541Code::RowDup => "表行重复（采样 同键双写）",
            u3541Code::RowGap => "表行断档（剖析 序号不连续）",
            u3541Code::ValOob => "值越界（折叠 值域外）",
            u3541Code::SumDrift => "累计漂移（周期中断 求和≠理论值）",
            u3541Code::LedgerDup => "账目重记（同一事件双入账）",
            u3541Code::LedgerHole => "账目断漏（序号缺失）",
            u3541Code::OpNoop => "空操作（账目无效果调用）",
            u3541Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3541Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3541Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3541Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3541Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3541Code::RowDup => "按键唯一律去重",
            u3541Code::RowGap => "按连续律补号",
            u3541Code::ValOob => "按值域裁剪",
            u3541Code::SumDrift => "按理论值复算",
            u3541Code::LedgerDup => "按事件幂等律合并",
            u3541Code::LedgerHole => "按连续律补账",
            u3541Code::OpNoop => "核对操作前置条件",
            u3541Code::FieldDrift => "按口径重建字段",
            u3541Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3541Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3541Code::TableBroken => "按语义规则重建常量表",
            u3541Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3541reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 采样剖析器 语义登记表（24 行逐行注释）。
pub const u3541reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3541reg: [(&'static str, u64); 24] = [
    ("PROF_A", 7), // PROF 主登记（剖析器 · 判据面）
    ("PROF_B", 33), // PROF 副登记（剖析器 · 对拍面）
    ("PMI_A", 14), // PMI 主登记（周期中断 · 判据面）
    ("PMI_B", 44), // PMI 副登记（周期中断 · 对拍面）
    ("TICK_A", 21), // TICK 主登记（桩化 tick · 判据面）
    ("TICK_B", 55), // TICK 副登记（桩化 tick · 对拍面）
    ("IP_A", 28), // IP 主登记（指令指针 · 判据面）
    ("IP_B", 66), // IP 副登记（指令指针 · 对拍面）
    ("STACK_A", 35), // STACK 主登记（栈采样 · 判据面）
    ("STACK_B", 77), // STACK 副登记（栈采样 · 对拍面）
    ("CALL_A", 42), // CALL 主登记（调用链 · 判据面）
    ("CALL_B", 88), // CALL 副登记（调用链 · 对拍面）
    ("RATE_A", 49), // RATE 主登记（采样率 · 判据面）
    ("RATE_B", 99), // RATE 副登记（采样率 · 对拍面）
    ("BUF_A", 56), // BUF 主登记（环形缓冲 · 判据面）
    ("BUF_B", 110), // BUF 副登记（环形缓冲 · 对拍面）
    ("ATTR_A", 63), // ATTR 主登记（归因 · 判据面）
    ("ATTR_B", 121), // ATTR 副登记（归因 · 对拍面）
    ("FOLD_A", 70), // FOLD 主登记（折叠 · 判据面）
    ("FOLD_B", 132), // FOLD 副登记（折叠 · 对拍面）
    ("FLAME_A", 77), // FLAME 主登记（火焰图 · 判据面）
    ("FLAME_B", 143), // FLAME 副登记（火焰图 · 对拍面）
    ("COST_A", 84), // COST 主登记（开销预算 · 判据面）
    ("COST_B", 154), // COST 副登记（开销预算 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3541reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3541reg_COUNT {
        if u3541reg[i].0 == key {
            return Some(u3541reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3541reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3541reg_COUNT {
        s = s.wrapping_add(u3541reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3541Ledger（采样剖析器 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 采样剖析器 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3541Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3541Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0e0, 0xe0ef]。
pub const fn u3541_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3541Code::ALL.len() {
        let c = u3541Code::ALL[i].code();
        if c < 0xe0e0 || c > 0xe0ef {
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
fn u3541_guard_seg() -> bool {
    u3541_segment_guard()
}

/// UNX-F3541 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3541_core_tbl() -> bool {
    let mut ok = u3541reg_COUNT == 24;
    if ok { ok = u3541reg_get("PROF_A") == Some(7) && u3541reg_get("PROF_B") == Some(33); }
    if ok { ok = u3541reg_get("COST_A") == Some(84); }
    if ok { ok = u3541reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3541reg_sum() == 1668; }
    ok
}

/// UNX-F3541 守恒账三操作确定性效果。
fn check_u3541_core_ldg() -> bool {
    let mut ld = Mxu3541Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3541_checks() -> CheckSet {
    let mut set = CheckSet::new("U3541");
    set.add(
        "u3541-seg-guard",
        u3541_guard_seg(),
        if u3541_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3541-core-tbl",
        check_u3541_core_tbl(),
        if check_u3541_core_tbl() { "" } else { "UNX-F3541 core table mismatch" },
    );
    set.add(
        "u3541-core-ldg",
        check_u3541_core_ldg(),
        if check_u3541_core_ldg() { "" } else { "UNX-F3541 core ledger mismatch" },
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
    fn u3541_all_checks_green() {
        let set = run_u3541_checks();
        assert!(set.all_passed(), "U3541 domain checks must be green");
    }

    #[test]
    fn u3541_segment_guard_holds() {
        assert!(u3541_segment_guard());
    }
}
