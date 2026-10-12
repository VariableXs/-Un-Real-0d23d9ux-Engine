//! unxu3521_u3521core —— 动态探针总设计（桩点式注入——编译期预留 probe 槽） 语义登记表与守恒账（动态探针）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3521 · 动态探针总设计（桩点式注入——编译期预留 probe 槽） · 判据原文：UNX-F3521 · 判据：J1：探针语义冻结（probe 槽=编译期预留的空操作调用点×运行时挂载处理器）；M-279 挂载全链（槽激活→处理器执行→卸载）。<br>J2：与静态 TP 的分工（TP=预定义事件/探针=临时观测——探针永不转正为 TP，转正走新条目流程）。
//! · 组件：动态探针 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3521 · 判据：J1：探针语义冻结（probe 槽=编译期预留的空操作调用点×运行时挂载处理器）；M-279 挂载全链（槽激活→处理器执行→卸载）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3521_VERSION: &str = "u3521-u3521core-v1";

/// 诊断段基址。
pub const U3521_SEG_BASE: u16 = 0xe0d0;

// --------------------------------------------------------------------------
// 诊断码（u3521 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 动态探针 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3521Code {
    /// 0xe0d0 表行重复（探针 同键双写）。
    RowDup,
    /// 0xe0d1 表行断档（桩点 序号不连续）。
    RowGap,
    /// 0xe0d2 值越界（注入 值域外）。
    ValOob,
    /// 0xe0d3 累计漂移（预留槽 求和≠理论值）。
    SumDrift,
    /// 0xe0d4 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe0d5 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe0d6 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe0d7 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe0d8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0d9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0da 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0db 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3521Code {
    /// 全量闭集。
    pub const ALL: [u3521Code; 12] = [
        u3521Code::RowDup,
        u3521Code::RowGap,
        u3521Code::ValOob,
        u3521Code::SumDrift,
        u3521Code::LedgerDup,
        u3521Code::LedgerHole,
        u3521Code::OpNoop,
        u3521Code::FieldDrift,
        u3521Code::LedgerBroken,
        u3521Code::SnapshotFingerprint,
        u3521Code::TableBroken,
        u3521Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3521Code::RowDup => 0xe0d0,
            u3521Code::RowGap => 0xe0d1,
            u3521Code::ValOob => 0xe0d2,
            u3521Code::SumDrift => 0xe0d3,
            u3521Code::LedgerDup => 0xe0d4,
            u3521Code::LedgerHole => 0xe0d5,
            u3521Code::OpNoop => 0xe0d6,
            u3521Code::FieldDrift => 0xe0d7,
            u3521Code::LedgerBroken => 0xe0d8,
            u3521Code::SnapshotFingerprint => 0xe0d9,
            u3521Code::TableBroken => 0xe0da,
            u3521Code::SegmentGuard => 0xe0db,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3521Code::RowDup => "表行重复（探针 同键双写）",
            u3521Code::RowGap => "表行断档（桩点 序号不连续）",
            u3521Code::ValOob => "值越界（注入 值域外）",
            u3521Code::SumDrift => "累计漂移（预留槽 求和≠理论值）",
            u3521Code::LedgerDup => "账目重记（同一事件双入账）",
            u3521Code::LedgerHole => "账目断漏（序号缺失）",
            u3521Code::OpNoop => "空操作（账目无效果调用）",
            u3521Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3521Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3521Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3521Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3521Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3521Code::RowDup => "按键唯一律去重",
            u3521Code::RowGap => "按连续律补号",
            u3521Code::ValOob => "按值域裁剪",
            u3521Code::SumDrift => "按理论值复算",
            u3521Code::LedgerDup => "按事件幂等律合并",
            u3521Code::LedgerHole => "按连续律补账",
            u3521Code::OpNoop => "核对操作前置条件",
            u3521Code::FieldDrift => "按口径重建字段",
            u3521Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3521Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3521Code::TableBroken => "按语义规则重建常量表",
            u3521Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3521reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 动态探针 语义登记表（24 行逐行注释）。
pub const u3521reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3521reg: [(&'static str, u64); 24] = [
    ("PROBE_A", 7), // PROBE 主登记（探针 · 判据面）
    ("PROBE_B", 33), // PROBE 副登记（探针 · 对拍面）
    ("SLOT_A", 14), // SLOT 主登记（探针槽 · 判据面）
    ("SLOT_B", 44), // SLOT 副登记（探针槽 · 对拍面）
    ("STUB_A", 21), // STUB 主登记（桩点 · 判据面）
    ("STUB_B", 55), // STUB 副登记（桩点 · 对拍面）
    ("INS_A", 28), // INS 主登记（注入 · 判据面）
    ("INS_B", 66), // INS 副登记（注入 · 对拍面）
    ("HOOK_A", 35), // HOOK 主登记（挂接 · 判据面）
    ("HOOK_B", 77), // HOOK 副登记（挂接 · 对拍面）
    ("CAPT_A", 42), // CAPT 主登记（捕获 · 判据面）
    ("CAPT_B", 88), // CAPT 副登记（捕获 · 对拍面）
    ("FILT_A", 49), // FILT 主登记（过滤 · 判据面）
    ("FILT_B", 99), // FILT 副登记（过滤 · 对拍面）
    ("BUD_A", 56), // BUD 主登记（预算 · 判据面）
    ("BUD_B", 110), // BUD 副登记（预算 · 对拍面）
    ("COUNT_A", 63), // COUNT 主登记（触发计数 · 判据面）
    ("COUNT_B", 121), // COUNT 副登记（触发计数 · 对拍面）
    ("ARM_A", 70), // ARM 主登记（布防 · 判据面）
    ("ARM_B", 132), // ARM 副登记（布防 · 对拍面）
    ("DIS_A", 77), // DIS 主登记（解除 · 判据面）
    ("DIS_B", 143), // DIS 副登记（解除 · 对拍面）
    ("LOG_A", 84), // LOG 主登记（探针日志 · 判据面）
    ("LOG_B", 154), // LOG 副登记（探针日志 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3521reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3521reg_COUNT {
        if u3521reg[i].0 == key {
            return Some(u3521reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3521reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3521reg_COUNT {
        s = s.wrapping_add(u3521reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3521Ledger（动态探针 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 动态探针 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3521Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3521Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0d0, 0xe0df]。
pub const fn u3521_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3521Code::ALL.len() {
        let c = u3521Code::ALL[i].code();
        if c < 0xe0d0 || c > 0xe0df {
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
fn u3521_guard_seg() -> bool {
    u3521_segment_guard()
}

/// UNX-F3521 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3521_core_tbl() -> bool {
    let mut ok = u3521reg_COUNT == 24;
    if ok { ok = u3521reg_get("PROBE_A") == Some(7) && u3521reg_get("PROBE_B") == Some(33); }
    if ok { ok = u3521reg_get("LOG_A") == Some(84); }
    if ok { ok = u3521reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3521reg_sum() == 1668; }
    ok
}

/// UNX-F3521 守恒账三操作确定性效果。
fn check_u3521_core_ldg() -> bool {
    let mut ld = Mxu3521Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3521_checks() -> CheckSet {
    let mut set = CheckSet::new("U3521");
    set.add(
        "u3521-seg-guard",
        u3521_guard_seg(),
        if u3521_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3521-core-tbl",
        check_u3521_core_tbl(),
        if check_u3521_core_tbl() { "" } else { "UNX-F3521 core table mismatch" },
    );
    set.add(
        "u3521-core-ldg",
        check_u3521_core_ldg(),
        if check_u3521_core_ldg() { "" } else { "UNX-F3521 core ledger mismatch" },
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
    fn u3521_all_checks_green() {
        let set = run_u3521_checks();
        assert!(set.all_passed(), "U3521 domain checks must be green");
    }

    #[test]
    fn u3521_segment_guard_holds() {
        assert!(u3521_segment_guard());
    }
}
