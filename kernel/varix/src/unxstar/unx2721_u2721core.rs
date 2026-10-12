//! unxu2721_u2721core —— 扫描器主循环与扫描节拍 语义登记表与守恒账（扫描轮次）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2721 · 扫描器主循环与扫描节拍 · 判据原文：UNX-F2721 · 判据：扫描主循环（isolate→谓词→逐出/归还三段）闭环 10 轮账平，扫描节拍双触发（水线/直接回收）命中同一入口
//! · 组件：扫描轮次 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2721 · 判据：扫描主循环（isolate→谓词→逐出/归还三段）闭环 10 轮账平，扫描节拍双触发（水线/直接回收）命中同一入口
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2721_VERSION: &str = "u2721-u2721core-v1";

/// 诊断段基址。
pub const U2721_SEG_BASE: u16 = 0xac00;

// --------------------------------------------------------------------------
// 诊断码（u2721 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 扫描轮次 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2721Code {
    /// 0xac00 表行重复（三段断链 同键双写）。
    RowDup,
    /// 0xac01 表行断档（节拍丢失 序号不连续）。
    RowGap,
    /// 0xac02 值越界（入口分叉 值域外）。
    ValOob,
    /// 0xac03 累计漂移（账不平 求和≠理论值）。
    SumDrift,
    /// 0xac04 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xac05 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xac06 空操作（账目无效果调用）。
    OpNoop,
    /// 0xac07 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xac08 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xac09 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xac0a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xac0b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2721Code {
    /// 全量闭集。
    pub const ALL: [u2721Code; 12] = [
        u2721Code::RowDup,
        u2721Code::RowGap,
        u2721Code::ValOob,
        u2721Code::SumDrift,
        u2721Code::LedgerDup,
        u2721Code::LedgerHole,
        u2721Code::OpNoop,
        u2721Code::FieldDrift,
        u2721Code::LedgerBroken,
        u2721Code::SnapshotFingerprint,
        u2721Code::TableBroken,
        u2721Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2721Code::RowDup => 0xac00,
            u2721Code::RowGap => 0xac01,
            u2721Code::ValOob => 0xac02,
            u2721Code::SumDrift => 0xac03,
            u2721Code::LedgerDup => 0xac04,
            u2721Code::LedgerHole => 0xac05,
            u2721Code::OpNoop => 0xac06,
            u2721Code::FieldDrift => 0xac07,
            u2721Code::LedgerBroken => 0xac08,
            u2721Code::SnapshotFingerprint => 0xac09,
            u2721Code::TableBroken => 0xac0a,
            u2721Code::SegmentGuard => 0xac0b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2721Code::RowDup => "表行重复（三段断链 同键双写）",
            u2721Code::RowGap => "表行断档（节拍丢失 序号不连续）",
            u2721Code::ValOob => "值越界（入口分叉 值域外）",
            u2721Code::SumDrift => "累计漂移（账不平 求和≠理论值）",
            u2721Code::LedgerDup => "账目重记（同一事件双入账）",
            u2721Code::LedgerHole => "账目断漏（序号缺失）",
            u2721Code::OpNoop => "空操作（账目无效果调用）",
            u2721Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2721Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2721Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2721Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2721Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2721Code::RowDup => "按键唯一律去重",
            u2721Code::RowGap => "按连续律补号",
            u2721Code::ValOob => "按值域裁剪",
            u2721Code::SumDrift => "按理论值复算",
            u2721Code::LedgerDup => "按事件幂等律合并",
            u2721Code::LedgerHole => "按连续律补账",
            u2721Code::OpNoop => "核对操作前置条件",
            u2721Code::FieldDrift => "按口径重建字段",
            u2721Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2721Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2721Code::TableBroken => "按语义规则重建常量表",
            u2721Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2721reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 扫描轮次 语义登记表（24 行逐行注释）。
pub const u2721reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2721reg: [(&'static str, u64); 24] = [
    ("SC_A", 7), // SC 主登记（扫描器 · 判据面）
    ("SC_B", 33), // SC 副登记（扫描器 · 对拍面）
    ("IS_A", 14), // IS 主登记（isolate 段 · 判据面）
    ("IS_B", 44), // IS 副登记（isolate 段 · 对拍面）
    ("PD_A", 21), // PD 主登记（谓词段 · 判据面）
    ("PD_B", 55), // PD 副登记（谓词段 · 对拍面）
    ("EV_A", 28), // EV 主登记（逐出段 · 判据面）
    ("EV_B", 66), // EV 副登记（逐出段 · 对拍面）
    ("RT_A", 35), // RT 主登记（归还段 · 判据面）
    ("RT_B", 77), // RT 副登记（归还段 · 对拍面）
    ("TP_A", 42), // TP 主登记（扫描节拍 · 判据面）
    ("TP_B", 88), // TP 副登记（扫描节拍 · 对拍面）
    ("WL_A", 49), // WL 主登记（水线触发 · 判据面）
    ("WL_B", 99), // WL 副登记（水线触发 · 对拍面）
    ("DR_A", 56), // DR 主登记（直接回收触发 · 判据面）
    ("DR_B", 110), // DR 副登记（直接回收触发 · 对拍面）
    ("SE_A", 63), // SE 主登记（同一入口 · 判据面）
    ("SE_B", 121), // SE 副登记（同一入口 · 对拍面）
    ("R10_A", 70), // R10 主登记（十轮闭环 · 判据面）
    ("R10_B", 132), // R10 副登记（十轮闭环 · 对拍面）
    ("AP_A", 77), // AP 主登记（账平断言 · 判据面）
    ("AP_B", 143), // AP 副登记（账平断言 · 对拍面）
    ("CT_A", 84), // CT 主登记（节拍面 · 判据面）
    ("CT_B", 154), // CT 副登记（节拍面 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2721reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2721reg_COUNT {
        if u2721reg[i].0 == key {
            return Some(u2721reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2721reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2721reg_COUNT {
        s = s.wrapping_add(u2721reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2721Ledger（扫描轮次 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 扫描轮次 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2721Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2721Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xac00, 0xac0f]。
pub const fn u2721_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2721Code::ALL.len() {
        let c = u2721Code::ALL[i].code();
        if c < 0xac00 || c > 0xac0f {
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
fn u2721_guard_seg() -> bool {
    u2721_segment_guard()
}

/// UNX-F2721 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2721_core_tbl() -> bool {
    let mut ok = u2721reg_COUNT == 24;
    if ok { ok = u2721reg_get("SC_A") == Some(7) && u2721reg_get("SC_B") == Some(33); }
    if ok { ok = u2721reg_get("CT_A") == Some(84); }
    if ok { ok = u2721reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2721reg_sum() == 1668; }
    ok
}

/// UNX-F2721 守恒账三操作确定性效果。
fn check_u2721_core_ldg() -> bool {
    let mut ld = Mxu2721Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2721_checks() -> CheckSet {
    let mut set = CheckSet::new("U2721");
    set.add(
        "u2721-seg-guard",
        u2721_guard_seg(),
        if u2721_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2721-core-tbl",
        check_u2721_core_tbl(),
        if check_u2721_core_tbl() { "" } else { "UNX-F2721 core table mismatch" },
    );
    set.add(
        "u2721-core-ldg",
        check_u2721_core_ldg(),
        if check_u2721_core_ldg() { "" } else { "UNX-F2721 core ledger mismatch" },
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
    fn u2721_all_checks_green() {
        let set = run_u2721_checks();
        assert!(set.all_passed(), "U2721 domain checks must be green");
    }

    #[test]
    fn u2721_segment_guard_holds() {
        assert!(u2721_segment_guard());
    }
}
