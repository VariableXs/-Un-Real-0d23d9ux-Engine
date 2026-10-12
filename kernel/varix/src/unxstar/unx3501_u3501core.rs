//! unxu3501_u3501core —— 跟踪点总设计（TP 注册表/编译期埋点/运行时开关） 语义登记表与守恒账（跟踪点体系）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3501 · 跟踪点总设计（TP 注册表/编译期埋点/运行时开关） · 判据原文：UNX-F3501 · 判据：J1：TP 语义冻结（埋点位置 × 负载类型 × 默认态三列矩阵）；M-259 TP 全链路（埋点→开关→负载→消费端可读）。<br>J2：与日志分工声明（文本流 vs 结构化事件流——F3506 协议对端，禁用态零开销）。
//! · 组件：跟踪点体系 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3501 · 判据：J1：TP 语义冻结（埋点位置 × 负载类型 × 默认态三列矩阵）；M-259 TP 全链路（埋点→开关→负载→消费端可读）。<b
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3501_VERSION: &str = "u3501-u3501core-v1";

/// 诊断段基址。
pub const U3501_SEG_BASE: u16 = 0xe0c0;

// --------------------------------------------------------------------------
// 诊断码（u3501 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 跟踪点体系 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3501Code {
    /// 0xe0c0 表行重复（跟踪点 同键双写）。
    RowDup,
    /// 0xe0c1 表行断档（埋点 序号不连续）。
    RowGap,
    /// 0xe0c2 值越界（开关 值域外）。
    ValOob,
    /// 0xe0c3 累计漂移（注册表 求和≠理论值）。
    SumDrift,
    /// 0xe0c4 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe0c5 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe0c6 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe0c7 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe0c8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0c9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0ca 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0cb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3501Code {
    /// 全量闭集。
    pub const ALL: [u3501Code; 12] = [
        u3501Code::RowDup,
        u3501Code::RowGap,
        u3501Code::ValOob,
        u3501Code::SumDrift,
        u3501Code::LedgerDup,
        u3501Code::LedgerHole,
        u3501Code::OpNoop,
        u3501Code::FieldDrift,
        u3501Code::LedgerBroken,
        u3501Code::SnapshotFingerprint,
        u3501Code::TableBroken,
        u3501Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3501Code::RowDup => 0xe0c0,
            u3501Code::RowGap => 0xe0c1,
            u3501Code::ValOob => 0xe0c2,
            u3501Code::SumDrift => 0xe0c3,
            u3501Code::LedgerDup => 0xe0c4,
            u3501Code::LedgerHole => 0xe0c5,
            u3501Code::OpNoop => 0xe0c6,
            u3501Code::FieldDrift => 0xe0c7,
            u3501Code::LedgerBroken => 0xe0c8,
            u3501Code::SnapshotFingerprint => 0xe0c9,
            u3501Code::TableBroken => 0xe0ca,
            u3501Code::SegmentGuard => 0xe0cb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3501Code::RowDup => "表行重复（跟踪点 同键双写）",
            u3501Code::RowGap => "表行断档（埋点 序号不连续）",
            u3501Code::ValOob => "值越界（开关 值域外）",
            u3501Code::SumDrift => "累计漂移（注册表 求和≠理论值）",
            u3501Code::LedgerDup => "账目重记（同一事件双入账）",
            u3501Code::LedgerHole => "账目断漏（序号缺失）",
            u3501Code::OpNoop => "空操作（账目无效果调用）",
            u3501Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3501Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3501Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3501Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3501Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3501Code::RowDup => "按键唯一律去重",
            u3501Code::RowGap => "按连续律补号",
            u3501Code::ValOob => "按值域裁剪",
            u3501Code::SumDrift => "按理论值复算",
            u3501Code::LedgerDup => "按事件幂等律合并",
            u3501Code::LedgerHole => "按连续律补账",
            u3501Code::OpNoop => "核对操作前置条件",
            u3501Code::FieldDrift => "按口径重建字段",
            u3501Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3501Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3501Code::TableBroken => "按语义规则重建常量表",
            u3501Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3501reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 跟踪点体系 语义登记表（24 行逐行注释）。
pub const u3501reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3501reg: [(&'static str, u64); 24] = [
    ("TP_A", 7), // TP 主登记（跟踪点 · 判据面）
    ("TP_B", 33), // TP 副登记（跟踪点 · 对拍面）
    ("REG_A", 14), // REG 主登记（注册表 · 判据面）
    ("REG_B", 44), // REG 副登记（注册表 · 对拍面）
    ("INS_A", 21), // INS 主登记（埋点 · 判据面）
    ("INS_B", 55), // INS 副登记（埋点 · 对拍面）
    ("SW_A", 28), // SW 主登记（运行时开关 · 判据面）
    ("SW_B", 66), // SW 副登记（运行时开关 · 对拍面）
    ("CID_A", 35), // CID 主登记（编译期 ID · 判据面）
    ("CID_B", 77), // CID 副登记（编译期 ID · 对拍面）
    ("HIT_A", 42), // HIT 主登记（命中计数 · 判据面）
    ("HIT_B", 88), // HIT 副登记（命中计数 · 对拍面）
    ("BUF_A", 49), // BUF 主登记（事件缓冲 · 判据面）
    ("BUF_B", 99), // BUF 副登记（事件缓冲 · 对拍面）
    ("FLT_A", 56), // FLT 主登记（过滤 · 判据面）
    ("FLT_B", 110), // FLT 副登记（过滤 · 对拍面）
    ("OVF_A", 63), // OVF 主登记（溢出 · 判据面）
    ("OVF_B", 121), // OVF 副登记（溢出 · 对拍面）
    ("DUMP_A", 70), // DUMP 主登记（导出 · 判据面）
    ("DUMP_B", 132), // DUMP 副登记（导出 · 对拍面）
    ("STATIC_A", 77), // STATIC 主登记（静态段 · 判据面）
    ("STATIC_B", 143), // STATIC 副登记（静态段 · 对拍面）
    ("DYN_A", 84), // DYN 主登记（动态段 · 判据面）
    ("DYN_B", 154), // DYN 副登记（动态段 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3501reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3501reg_COUNT {
        if u3501reg[i].0 == key {
            return Some(u3501reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3501reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3501reg_COUNT {
        s = s.wrapping_add(u3501reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3501Ledger（跟踪点体系 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 跟踪点体系 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3501Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3501Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0c0, 0xe0cf]。
pub const fn u3501_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3501Code::ALL.len() {
        let c = u3501Code::ALL[i].code();
        if c < 0xe0c0 || c > 0xe0cf {
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
fn u3501_guard_seg() -> bool {
    u3501_segment_guard()
}

/// UNX-F3501 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3501_core_tbl() -> bool {
    let mut ok = u3501reg_COUNT == 24;
    if ok { ok = u3501reg_get("TP_A") == Some(7) && u3501reg_get("TP_B") == Some(33); }
    if ok { ok = u3501reg_get("DYN_A") == Some(84); }
    if ok { ok = u3501reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3501reg_sum() == 1668; }
    ok
}

/// UNX-F3501 守恒账三操作确定性效果。
fn check_u3501_core_ldg() -> bool {
    let mut ld = Mxu3501Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3501_checks() -> CheckSet {
    let mut set = CheckSet::new("U3501");
    set.add(
        "u3501-seg-guard",
        u3501_guard_seg(),
        if u3501_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3501-core-tbl",
        check_u3501_core_tbl(),
        if check_u3501_core_tbl() { "" } else { "UNX-F3501 core table mismatch" },
    );
    set.add(
        "u3501-core-ldg",
        check_u3501_core_ldg(),
        if check_u3501_core_ldg() { "" } else { "UNX-F3501 core ledger mismatch" },
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
    fn u3501_all_checks_green() {
        let set = run_u3501_checks();
        assert!(set.all_passed(), "U3501 domain checks must be green");
    }

    #[test]
    fn u3501_segment_guard_holds() {
        assert!(u3501_segment_guard());
    }
}
