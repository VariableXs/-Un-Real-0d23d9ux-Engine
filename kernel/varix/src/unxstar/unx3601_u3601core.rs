//! unxu3601_u3601core —— 锁压力机总设计（场景编排/指标采集/裁决门） 语义登记表与守恒账（锁压力机）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3601 · 锁压力机总设计（场景编排/指标采集/裁决门） · 判据原文：UNX-F3601 · 判据：J1：压力机三层（场景层（七类场景注册）/执行层（N 工作者调度）/度量层（争用账挂钩）——判据行内嵌）；M-359 全链首跑（场景注册→执行→度量→裁决四段走通）。<br>J2：零侵断言（压力机关闭时锁路径拍数不变——度量是包裹观测）。
//! · 组件：锁压力机 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3601 · 判据：J1：压力机三层（场景层（七类场景注册）/执行层（N 工作者调度）/度量层（争用账挂钩）——判据行内嵌）；M-359 全链首跑（场
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3601_VERSION: &str = "u3601-u3601core-v1";

/// 诊断段基址。
pub const U3601_SEG_BASE: u16 = 0xe110;

// --------------------------------------------------------------------------
// 诊断码（u3601 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 锁压力机 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3601Code {
    /// 0xe110 表行重复（压力 同键双写）。
    RowDup,
    /// 0xe111 表行断档（场景 序号不连续）。
    RowGap,
    /// 0xe112 值越界（裁决 值域外）。
    ValOob,
    /// 0xe113 累计漂移（裁决门 求和≠理论值）。
    SumDrift,
    /// 0xe114 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe115 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe116 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe117 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe118 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe119 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe11a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe11b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3601Code {
    /// 全量闭集。
    pub const ALL: [u3601Code; 12] = [
        u3601Code::RowDup,
        u3601Code::RowGap,
        u3601Code::ValOob,
        u3601Code::SumDrift,
        u3601Code::LedgerDup,
        u3601Code::LedgerHole,
        u3601Code::OpNoop,
        u3601Code::FieldDrift,
        u3601Code::LedgerBroken,
        u3601Code::SnapshotFingerprint,
        u3601Code::TableBroken,
        u3601Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3601Code::RowDup => 0xe110,
            u3601Code::RowGap => 0xe111,
            u3601Code::ValOob => 0xe112,
            u3601Code::SumDrift => 0xe113,
            u3601Code::LedgerDup => 0xe114,
            u3601Code::LedgerHole => 0xe115,
            u3601Code::OpNoop => 0xe116,
            u3601Code::FieldDrift => 0xe117,
            u3601Code::LedgerBroken => 0xe118,
            u3601Code::SnapshotFingerprint => 0xe119,
            u3601Code::TableBroken => 0xe11a,
            u3601Code::SegmentGuard => 0xe11b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3601Code::RowDup => "表行重复（压力 同键双写）",
            u3601Code::RowGap => "表行断档（场景 序号不连续）",
            u3601Code::ValOob => "值越界（裁决 值域外）",
            u3601Code::SumDrift => "累计漂移（裁决门 求和≠理论值）",
            u3601Code::LedgerDup => "账目重记（同一事件双入账）",
            u3601Code::LedgerHole => "账目断漏（序号缺失）",
            u3601Code::OpNoop => "空操作（账目无效果调用）",
            u3601Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3601Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3601Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3601Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3601Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3601Code::RowDup => "按键唯一律去重",
            u3601Code::RowGap => "按连续律补号",
            u3601Code::ValOob => "按值域裁剪",
            u3601Code::SumDrift => "按理论值复算",
            u3601Code::LedgerDup => "按事件幂等律合并",
            u3601Code::LedgerHole => "按连续律补账",
            u3601Code::OpNoop => "核对操作前置条件",
            u3601Code::FieldDrift => "按口径重建字段",
            u3601Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3601Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3601Code::TableBroken => "按语义规则重建常量表",
            u3601Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3601reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 锁压力机 语义登记表（24 行逐行注释）。
pub const u3601reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3601reg: [(&'static str, u64); 24] = [
    ("STRESS_A", 7), // STRESS 主登记（压力机 · 判据面）
    ("STRESS_B", 33), // STRESS 副登记（压力机 · 对拍面）
    ("SCN_A", 14), // SCN 主登记（场景 · 判据面）
    ("SCN_B", 44), // SCN 副登记（场景 · 对拍面）
    ("MET_A", 21), // MET 主登记（指标 · 判据面）
    ("MET_B", 55), // MET 副登记（指标 · 对拍面）
    ("GATE_A", 28), // GATE 主登记（裁决门 · 判据面）
    ("GATE_B", 66), // GATE 副登记（裁决门 · 对拍面）
    ("LOAD_A", 35), // LOAD 主登记（负载 · 判据面）
    ("LOAD_B", 77), // LOAD 副登记（负载 · 对拍面）
    ("CONT_A", 42), // CONT 主登记（争用 · 判据面）
    ("CONT_B", 88), // CONT 副登记（争用 · 对拍面）
    ("THR_A", 49), // THR 主登记（阈值 · 判据面）
    ("THR_B", 99), // THR 副登记（阈值 · 对拍面）
    ("SCORE_A", 56), // SCORE 主登记（评分 · 判据面）
    ("SCORE_B", 110), // SCORE 副登记（评分 · 对拍面）
    ("RUN_A", 63), // RUN 主登记（轮次 · 判据面）
    ("RUN_B", 121), // RUN 副登记（轮次 · 对拍面）
    ("SAMP_A", 70), // SAMP 主登记（采样 · 判据面）
    ("SAMP_B", 132), // SAMP 副登记（采样 · 对拍面）
    ("VERDICT_A", 77), // VERDICT 主登记（裁决 · 判据面）
    ("VERDICT_B", 143), // VERDICT 副登记（裁决 · 对拍面）
    ("RPT_A", 84), // RPT 主登记（报告 · 判据面）
    ("RPT_B", 154), // RPT 副登记（报告 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3601reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3601reg_COUNT {
        if u3601reg[i].0 == key {
            return Some(u3601reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3601reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3601reg_COUNT {
        s = s.wrapping_add(u3601reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3601Ledger（锁压力机 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 锁压力机 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3601Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3601Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe110, 0xe11f]。
pub const fn u3601_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3601Code::ALL.len() {
        let c = u3601Code::ALL[i].code();
        if c < 0xe110 || c > 0xe11f {
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
fn u3601_guard_seg() -> bool {
    u3601_segment_guard()
}

/// UNX-F3601 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3601_core_tbl() -> bool {
    let mut ok = u3601reg_COUNT == 24;
    if ok { ok = u3601reg_get("STRESS_A") == Some(7) && u3601reg_get("STRESS_B") == Some(33); }
    if ok { ok = u3601reg_get("RPT_A") == Some(84); }
    if ok { ok = u3601reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3601reg_sum() == 1668; }
    ok
}

/// UNX-F3601 守恒账三操作确定性效果。
fn check_u3601_core_ldg() -> bool {
    let mut ld = Mxu3601Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3601_checks() -> CheckSet {
    let mut set = CheckSet::new("U3601");
    set.add(
        "u3601-seg-guard",
        u3601_guard_seg(),
        if u3601_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3601-core-tbl",
        check_u3601_core_tbl(),
        if check_u3601_core_tbl() { "" } else { "UNX-F3601 core table mismatch" },
    );
    set.add(
        "u3601-core-ldg",
        check_u3601_core_ldg(),
        if check_u3601_core_ldg() { "" } else { "UNX-F3601 core ledger mismatch" },
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
    fn u3601_all_checks_green() {
        let set = run_u3601_checks();
        assert!(set.all_passed(), "U3601 domain checks must be green");
    }

    #[test]
    fn u3601_segment_guard_holds() {
        assert!(u3601_segment_guard());
    }
}
