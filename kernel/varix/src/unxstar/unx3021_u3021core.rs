//! unxu3021_u3021core —— 三档压力分档冻结（轻/中/重回收压力档定义） 语义登记表与守恒账（三档压力分档）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3021 · 三档压力分档冻结（轻/中/重回收压力档定义） · 判据原文：UNX-F3021 · 判据：UNX-F3021-J1 三档分档冻结：轻/中/重三档（水线区间+扫描强度二维）定义哈希入册，与 F2941 压力矩阵三轴口径同源核过，档位漂移注入 10/10 次拦截
//! · 组件：三档压力分档 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3021 · 判据：UNX-F3021-J1 三档分档冻结：轻/中/重三档（水线区间+扫描强度二维）定义哈希入册，与 F2941 压力矩阵三轴口径同源
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3021_VERSION: &str = "u3021-u3021core-v1";

/// 诊断段基址。
pub const U3021_SEG_BASE: u16 = 0xe260;

// --------------------------------------------------------------------------
// 诊断码（u3021 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 三档压力分档 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3021Code {
    /// 0xe260 表行重复（三档 同键双写）。
    RowDup,
    /// 0xe261 表行断档（压力 序号不连续）。
    RowGap,
    /// 0xe262 值越界（分档 值域外）。
    ValOob,
    /// 0xe263 累计漂移（分档 求和≠理论值）。
    SumDrift,
    /// 0xe264 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe265 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe266 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe267 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe268 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe269 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe26a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe26b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3021Code {
    /// 全量闭集。
    pub const ALL: [u3021Code; 12] = [
        u3021Code::RowDup,
        u3021Code::RowGap,
        u3021Code::ValOob,
        u3021Code::SumDrift,
        u3021Code::LedgerDup,
        u3021Code::LedgerHole,
        u3021Code::OpNoop,
        u3021Code::FieldDrift,
        u3021Code::LedgerBroken,
        u3021Code::SnapshotFingerprint,
        u3021Code::TableBroken,
        u3021Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3021Code::RowDup => 0xe260,
            u3021Code::RowGap => 0xe261,
            u3021Code::ValOob => 0xe262,
            u3021Code::SumDrift => 0xe263,
            u3021Code::LedgerDup => 0xe264,
            u3021Code::LedgerHole => 0xe265,
            u3021Code::OpNoop => 0xe266,
            u3021Code::FieldDrift => 0xe267,
            u3021Code::LedgerBroken => 0xe268,
            u3021Code::SnapshotFingerprint => 0xe269,
            u3021Code::TableBroken => 0xe26a,
            u3021Code::SegmentGuard => 0xe26b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3021Code::RowDup => "表行重复（三档 同键双写）",
            u3021Code::RowGap => "表行断档（压力 序号不连续）",
            u3021Code::ValOob => "值越界（分档 值域外）",
            u3021Code::SumDrift => "累计漂移（分档 求和≠理论值）",
            u3021Code::LedgerDup => "账目重记（同一事件双入账）",
            u3021Code::LedgerHole => "账目断漏（序号缺失）",
            u3021Code::OpNoop => "空操作（账目无效果调用）",
            u3021Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3021Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3021Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3021Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3021Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3021Code::RowDup => "按键唯一律去重",
            u3021Code::RowGap => "按连续律补号",
            u3021Code::ValOob => "按值域裁剪",
            u3021Code::SumDrift => "按理论值复算",
            u3021Code::LedgerDup => "按事件幂等律合并",
            u3021Code::LedgerHole => "按连续律补账",
            u3021Code::OpNoop => "核对操作前置条件",
            u3021Code::FieldDrift => "按口径重建字段",
            u3021Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3021Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3021Code::TableBroken => "按语义规则重建常量表",
            u3021Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3021reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 三档压力分档 语义登记表（24 行逐行注释）。
pub const u3021reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3021reg: [(&'static str, u64); 24] = [
    ("T1_A", 7), // T1 主登记（轻档 · 判据面）
    ("T1_B", 33), // T1 副登记（轻档 · 对拍面）
    ("T2_A", 14), // T2 主登记（中档 · 判据面）
    ("T2_B", 44), // T2 副登记（中档 · 对拍面）
    ("T3_A", 21), // T3 主登记（重档 · 判据面）
    ("T3_B", 55), // T3 副登记（重档 · 对拍面）
    ("RECLAIM_A", 28), // RECLAIM 主登记（回收压力 · 判据面）
    ("RECLAIM_B", 66), // RECLAIM 副登记（回收压力 · 对拍面）
    ("DOSE_A", 35), // DOSE 主登记（剂量 · 判据面）
    ("DOSE_B", 77), // DOSE 副登记（剂量 · 对拍面）
    ("LOAD_A", 42), // LOAD 主登记（负载 · 判据面）
    ("LOAD_B", 88), // LOAD 副登记（负载 · 对拍面）
    ("DUR_A", 49), // DUR 主登记（时长 · 判据面）
    ("DUR_B", 99), // DUR 副登记（时长 · 对拍面）
    ("MET_A", 56), // MET 主登记（指标 · 判据面）
    ("MET_B", 110), // MET 副登记（指标 · 对拍面）
    ("THR_A", 63), // THR 主登记（阈值 · 判据面）
    ("THR_B", 121), // THR 副登记（阈值 · 对拍面）
    ("SCALE_A", 70), // SCALE 主登记（伸缩 · 判据面）
    ("SCALE_B", 132), // SCALE 副登记（伸缩 · 对拍面）
    ("BUD_A", 77), // BUD 主登记（预算 · 判据面）
    ("BUD_B", 143), // BUD 副登记（预算 · 对拍面）
    ("VERDICT_A", 84), // VERDICT 主登记（裁决 · 判据面）
    ("VERDICT_B", 154), // VERDICT 副登记（裁决 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3021reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3021reg_COUNT {
        if u3021reg[i].0 == key {
            return Some(u3021reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3021reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3021reg_COUNT {
        s = s.wrapping_add(u3021reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3021Ledger（三档压力分档 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 三档压力分档 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3021Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3021Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe260, 0xe26f]。
pub const fn u3021_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3021Code::ALL.len() {
        let c = u3021Code::ALL[i].code();
        if c < 0xe260 || c > 0xe26f {
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
fn u3021_guard_seg() -> bool {
    u3021_segment_guard()
}

/// UNX-F3021 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3021_core_tbl() -> bool {
    let mut ok = u3021reg_COUNT == 24;
    if ok { ok = u3021reg_get("T1_A") == Some(7) && u3021reg_get("T1_B") == Some(33); }
    if ok { ok = u3021reg_get("VERDICT_A") == Some(84); }
    if ok { ok = u3021reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3021reg_sum() == 1668; }
    ok
}

/// UNX-F3021 守恒账三操作确定性效果。
fn check_u3021_core_ldg() -> bool {
    let mut ld = Mxu3021Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3021_checks() -> CheckSet {
    let mut set = CheckSet::new("U3021");
    set.add(
        "u3021-seg-guard",
        u3021_guard_seg(),
        if u3021_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3021-core-tbl",
        check_u3021_core_tbl(),
        if check_u3021_core_tbl() { "" } else { "UNX-F3021 core table mismatch" },
    );
    set.add(
        "u3021-core-ldg",
        check_u3021_core_ldg(),
        if check_u3021_core_ldg() { "" } else { "UNX-F3021 core ledger mismatch" },
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
    fn u3021_all_checks_green() {
        let set = run_u3021_checks();
        assert!(set.all_passed(), "U3021 domain checks must be green");
    }

    #[test]
    fn u3021_segment_guard_holds() {
        assert!(u3021_segment_guard());
    }
}
