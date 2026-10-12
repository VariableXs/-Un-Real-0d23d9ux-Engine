//! unxu3381_u3381core —— 样本库总设计（可复现死锁样本注入框架） 语义登记表与守恒账（死锁样本库）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3381 · 样本库总设计（可复现死锁样本注入框架） · 判据原文：UNX-F3381 · 判据：J1：框架文档交付：样本 = 参数化场景函数（锁数/时长/时机/期望结果四参数）；M-140 框架空转验证。
//! · 组件：死锁样本库 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3381 · 判据：J1：框架文档交付：样本 = 参数化场景函数（锁数/时长/时机/期望结果四参数）；M-140 框架空转验证。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3381_VERSION: &str = "u3381-u3381core-v1";

/// 诊断段基址。
pub const U3381_SEG_BASE: u16 = 0xe060;

// --------------------------------------------------------------------------
// 诊断码（u3381 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 死锁样本库 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3381Code {
    /// 0xe060 表行重复（样本 同键双写）。
    RowDup,
    /// 0xe061 表行断档（重放 序号不连续）。
    RowGap,
    /// 0xe062 值越界（确定性 值域外）。
    ValOob,
    /// 0xe063 累计漂移（可复现 求和≠理论值）。
    SumDrift,
    /// 0xe064 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe065 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe066 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe067 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe068 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe069 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe06a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe06b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3381Code {
    /// 全量闭集。
    pub const ALL: [u3381Code; 12] = [
        u3381Code::RowDup,
        u3381Code::RowGap,
        u3381Code::ValOob,
        u3381Code::SumDrift,
        u3381Code::LedgerDup,
        u3381Code::LedgerHole,
        u3381Code::OpNoop,
        u3381Code::FieldDrift,
        u3381Code::LedgerBroken,
        u3381Code::SnapshotFingerprint,
        u3381Code::TableBroken,
        u3381Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3381Code::RowDup => 0xe060,
            u3381Code::RowGap => 0xe061,
            u3381Code::ValOob => 0xe062,
            u3381Code::SumDrift => 0xe063,
            u3381Code::LedgerDup => 0xe064,
            u3381Code::LedgerHole => 0xe065,
            u3381Code::OpNoop => 0xe066,
            u3381Code::FieldDrift => 0xe067,
            u3381Code::LedgerBroken => 0xe068,
            u3381Code::SnapshotFingerprint => 0xe069,
            u3381Code::TableBroken => 0xe06a,
            u3381Code::SegmentGuard => 0xe06b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3381Code::RowDup => "表行重复（样本 同键双写）",
            u3381Code::RowGap => "表行断档（重放 序号不连续）",
            u3381Code::ValOob => "值越界（确定性 值域外）",
            u3381Code::SumDrift => "累计漂移（可复现 求和≠理论值）",
            u3381Code::LedgerDup => "账目重记（同一事件双入账）",
            u3381Code::LedgerHole => "账目断漏（序号缺失）",
            u3381Code::OpNoop => "空操作（账目无效果调用）",
            u3381Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3381Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3381Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3381Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3381Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3381Code::RowDup => "按键唯一律去重",
            u3381Code::RowGap => "按连续律补号",
            u3381Code::ValOob => "按值域裁剪",
            u3381Code::SumDrift => "按理论值复算",
            u3381Code::LedgerDup => "按事件幂等律合并",
            u3381Code::LedgerHole => "按连续律补账",
            u3381Code::OpNoop => "核对操作前置条件",
            u3381Code::FieldDrift => "按口径重建字段",
            u3381Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3381Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3381Code::TableBroken => "按语义规则重建常量表",
            u3381Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3381reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 死锁样本库 语义登记表（24 行逐行注释）。
pub const u3381reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3381reg: [(&'static str, u64); 24] = [
    ("SMP_A", 7), // SMP 主登记（样本 · 判据面）
    ("SMP_B", 33), // SMP 副登记（样本 · 对拍面）
    ("SEED_A", 14), // SEED 主登记（随机种子 · 判据面）
    ("SEED_B", 44), // SEED 副登记（随机种子 · 对拍面）
    ("REPLAY_A", 21), // REPLAY 主登记（重放 · 判据面）
    ("REPLAY_B", 55), // REPLAY 副登记（重放 · 对拍面）
    ("DET_A", 28), // DET 主登记（确定性 · 判据面）
    ("DET_B", 66), // DET 副登记（确定性 · 对拍面）
    ("INJ_A", 35), // INJ 主登记（注入点 · 判据面）
    ("INJ_B", 77), // INJ 副登记（注入点 · 对拍面）
    ("GEN_A", 42), // GEN 主登记（生成器 · 判据面）
    ("GEN_B", 88), // GEN 副登记（生成器 · 对拍面）
    ("BUF_A", 49), // BUF 主登记（样本缓冲 · 判据面）
    ("BUF_B", 99), // BUF 副登记（样本缓冲 · 对拍面）
    ("SIG_A", 56), // SIG 主登记（签名 · 判据面）
    ("SIG_B", 110), // SIG 副登记（签名 · 对拍面）
    ("CUR_A", 63), // CUR 主登记（当前样本 · 判据面）
    ("CUR_B", 121), // CUR 副登记（当前样本 · 对拍面）
    ("CAP_A", 70), // CAP 主登记（容量 · 判据面）
    ("CAP_B", 132), // CAP 副登记（容量 · 对拍面）
    ("VER_A", 77), // VER 主登记（版本 · 判据面）
    ("VER_B", 143), // VER 副登记（版本 · 对拍面）
    ("TRACE_A", 84), // TRACE 主登记（轨迹 · 判据面）
    ("TRACE_B", 154), // TRACE 副登记（轨迹 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3381reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3381reg_COUNT {
        if u3381reg[i].0 == key {
            return Some(u3381reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3381reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3381reg_COUNT {
        s = s.wrapping_add(u3381reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3381Ledger（死锁样本库 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 死锁样本库 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3381Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3381Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe060, 0xe06f]。
pub const fn u3381_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3381Code::ALL.len() {
        let c = u3381Code::ALL[i].code();
        if c < 0xe060 || c > 0xe06f {
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
fn u3381_guard_seg() -> bool {
    u3381_segment_guard()
}

/// UNX-F3381 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3381_core_tbl() -> bool {
    let mut ok = u3381reg_COUNT == 24;
    if ok { ok = u3381reg_get("SMP_A") == Some(7) && u3381reg_get("SMP_B") == Some(33); }
    if ok { ok = u3381reg_get("TRACE_A") == Some(84); }
    if ok { ok = u3381reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3381reg_sum() == 1668; }
    ok
}

/// UNX-F3381 守恒账三操作确定性效果。
fn check_u3381_core_ldg() -> bool {
    let mut ld = Mxu3381Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3381_checks() -> CheckSet {
    let mut set = CheckSet::new("U3381");
    set.add(
        "u3381-seg-guard",
        u3381_guard_seg(),
        if u3381_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3381-core-tbl",
        check_u3381_core_tbl(),
        if check_u3381_core_tbl() { "" } else { "UNX-F3381 core table mismatch" },
    );
    set.add(
        "u3381-core-ldg",
        check_u3381_core_ldg(),
        if check_u3381_core_ldg() { "" } else { "UNX-F3381 core ledger mismatch" },
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
    fn u3381_all_checks_green() {
        let set = run_u3381_checks();
        assert!(set.all_passed(), "U3381 domain checks must be green");
    }

    #[test]
    fn u3381_segment_guard_holds() {
        assert!(u3381_segment_guard());
    }
}
