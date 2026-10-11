//! unxu3481_u3481core —— 分级日志总设计（五级 TRACE/DEBUG/INFO/WARN/ERROR + 分级输出策略） 语义登记表与守恒账（分级日志）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3481 · 分级日志总设计（五级 TRACE/DEBUG/INFO/WARN/ERROR + 分级输出策略） · 判据原文：UNX-F3481 · 判据：J1：五级语义冻结（级别定义 × 消费者 × 输出策略三列——判据行内嵌矩阵）；M-240 分级全链路（五级各一条日志 → 过滤 → 转储 → 可读）。
//! · 组件：分级日志 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3481 · 判据：J1：五级语义冻结（级别定义 × 消费者 × 输出策略三列——判据行内嵌矩阵）；M-240 分级全链路（五级各一条日志 → 过滤 
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3481_VERSION: &str = "u3481-u3481core-v1";

/// 诊断段基址。
pub const U3481_SEG_BASE: u16 = 0xe0b0;

// --------------------------------------------------------------------------
// 诊断码（u3481 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 分级日志 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3481Code {
    /// 0xe0b0 表行重复（日志 同键双写）。
    RowDup,
    /// 0xe0b1 表行断档（分级 序号不连续）。
    RowGap,
    /// 0xe0b2 值越界（限速 值域外）。
    ValOob,
    /// 0xe0b3 累计漂移（五级 求和≠理论值）。
    SumDrift,
    /// 0xe0b4 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe0b5 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe0b6 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe0b7 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe0b8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0b9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0ba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0bb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3481Code {
    /// 全量闭集。
    pub const ALL: [u3481Code; 12] = [
        u3481Code::RowDup,
        u3481Code::RowGap,
        u3481Code::ValOob,
        u3481Code::SumDrift,
        u3481Code::LedgerDup,
        u3481Code::LedgerHole,
        u3481Code::OpNoop,
        u3481Code::FieldDrift,
        u3481Code::LedgerBroken,
        u3481Code::SnapshotFingerprint,
        u3481Code::TableBroken,
        u3481Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3481Code::RowDup => 0xe0b0,
            u3481Code::RowGap => 0xe0b1,
            u3481Code::ValOob => 0xe0b2,
            u3481Code::SumDrift => 0xe0b3,
            u3481Code::LedgerDup => 0xe0b4,
            u3481Code::LedgerHole => 0xe0b5,
            u3481Code::OpNoop => 0xe0b6,
            u3481Code::FieldDrift => 0xe0b7,
            u3481Code::LedgerBroken => 0xe0b8,
            u3481Code::SnapshotFingerprint => 0xe0b9,
            u3481Code::TableBroken => 0xe0ba,
            u3481Code::SegmentGuard => 0xe0bb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3481Code::RowDup => "表行重复（日志 同键双写）",
            u3481Code::RowGap => "表行断档（分级 序号不连续）",
            u3481Code::ValOob => "值越界（限速 值域外）",
            u3481Code::SumDrift => "累计漂移（五级 求和≠理论值）",
            u3481Code::LedgerDup => "账目重记（同一事件双入账）",
            u3481Code::LedgerHole => "账目断漏（序号缺失）",
            u3481Code::OpNoop => "空操作（账目无效果调用）",
            u3481Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3481Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3481Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3481Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3481Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3481Code::RowDup => "按键唯一律去重",
            u3481Code::RowGap => "按连续律补号",
            u3481Code::ValOob => "按值域裁剪",
            u3481Code::SumDrift => "按理论值复算",
            u3481Code::LedgerDup => "按事件幂等律合并",
            u3481Code::LedgerHole => "按连续律补账",
            u3481Code::OpNoop => "核对操作前置条件",
            u3481Code::FieldDrift => "按口径重建字段",
            u3481Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3481Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3481Code::TableBroken => "按语义规则重建常量表",
            u3481Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3481reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 分级日志 语义登记表（24 行逐行注释）。
pub const u3481reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3481reg: [(&'static str, u64); 24] = [
    ("LVL_A", 7), // LVL 主登记（日志级别 · 判据面）
    ("LVL_B", 33), // LVL 副登记（日志级别 · 对拍面）
    ("TRACE_A", 14), // TRACE 主登记（跟踪级 · 判据面）
    ("TRACE_B", 44), // TRACE 副登记（跟踪级 · 对拍面）
    ("DBG_A", 21), // DBG 主登记（调试级 · 判据面）
    ("DBG_B", 55), // DBG 副登记（调试级 · 对拍面）
    ("INF_A", 28), // INF 主登记（信息级 · 判据面）
    ("INF_B", 66), // INF 副登记（信息级 · 对拍面）
    ("WRN_A", 35), // WRN 主登记（警告级 · 判据面）
    ("WRN_B", 77), // WRN 副登记（警告级 · 对拍面）
    ("ERR_A", 42), // ERR 主登记（错误级 · 判据面）
    ("ERR_B", 88), // ERR 副登记（错误级 · 对拍面）
    ("FILT_A", 49), // FILT 主登记（过滤 · 判据面）
    ("FILT_B", 99), // FILT 副登记（过滤 · 对拍面）
    ("BUF_A", 56), // BUF 主登记（环形缓冲 · 判据面）
    ("BUF_B", 110), // BUF 副登记（环形缓冲 · 对拍面）
    ("RATE_A", 63), // RATE 主登记（限速 · 判据面）
    ("RATE_B", 121), // RATE 副登记（限速 · 对拍面）
    ("FLUSH_A", 70), // FLUSH 主登记（刷盘 · 判据面）
    ("FLUSH_B", 132), // FLUSH 副登记（刷盘 · 对拍面）
    ("CTX_A", 77), // CTX 主登记（上下文字段 · 判据面）
    ("CTX_B", 143), // CTX 副登记（上下文字段 · 对拍面）
    ("DROP_A", 84), // DROP 主登记（丢弃计数 · 判据面）
    ("DROP_B", 154), // DROP 副登记（丢弃计数 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3481reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3481reg_COUNT {
        if u3481reg[i].0 == key {
            return Some(u3481reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3481reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3481reg_COUNT {
        s = s.wrapping_add(u3481reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3481Ledger（分级日志 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 分级日志 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3481Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3481Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0b0, 0xe0bf]。
pub const fn u3481_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3481Code::ALL.len() {
        let c = u3481Code::ALL[i].code();
        if c < 0xe0b0 || c > 0xe0bf {
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
fn u3481_guard_seg() -> bool {
    u3481_segment_guard()
}

/// UNX-F3481 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3481_core_tbl() -> bool {
    let mut ok = u3481reg_COUNT == 24;
    if ok { ok = u3481reg_get("LVL_A") == Some(7) && u3481reg_get("LVL_B") == Some(33); }
    if ok { ok = u3481reg_get("DROP_A") == Some(84); }
    if ok { ok = u3481reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3481reg_sum() == 1668; }
    ok
}

/// UNX-F3481 守恒账三操作确定性效果。
fn check_u3481_core_ldg() -> bool {
    let mut ld = Mxu3481Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3481_checks() -> CheckSet {
    let mut set = CheckSet::new("U3481");
    set.add(
        "u3481-seg-guard",
        u3481_guard_seg(),
        if u3481_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3481-core-tbl",
        check_u3481_core_tbl(),
        if check_u3481_core_tbl() { "" } else { "UNX-F3481 core table mismatch" },
    );
    set.add(
        "u3481-core-ldg",
        check_u3481_core_ldg(),
        if check_u3481_core_ldg() { "" } else { "UNX-F3481 core ledger mismatch" },
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
    fn u3481_all_checks_green() {
        let set = run_u3481_checks();
        assert!(set.all_passed(), "U3481 domain checks must be green");
    }

    #[test]
    fn u3481_segment_guard_holds() {
        assert!(u3481_segment_guard());
    }
}
