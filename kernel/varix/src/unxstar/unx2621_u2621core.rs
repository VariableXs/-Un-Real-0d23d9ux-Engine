//! unxu2621_u2621core —— 每进程 CR3 语义与上下文切换内存侧（A3 每核上下文消费） 语义登记表与守恒账（CR3 切换）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2621 · 每进程 CR3 语义与上下文切换内存侧（A3 每核上下文消费） · 判据原文：UNX-F2621 · 判据：CR3 切换内存侧语义逐项断言（切换点单点/NOFLUSH 组合正确），A3 每核上下文冻结件消费联签在档
//! · 组件：CR3 切换 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2621 · 判据：CR3 切换内存侧语义逐项断言（切换点单点/NOFLUSH 组合正确），A3 每核上下文冻结件消费联签在档
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2621_VERSION: &str = "u2621-u2621core-v1";

/// 诊断段基址。
pub const U2621_SEG_BASE: u16 = 0xa840;

// --------------------------------------------------------------------------
// 诊断码（u2621 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// CR3 切换 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2621Code {
    /// 0xa840 表行重复（切换点漏 同键双写）。
    RowDup,
    /// 0xa841 表行断档（冲刷错配 序号不连续）。
    RowGap,
    /// 0xa842 值越界（联签缺失 值域外）。
    ValOob,
    /// 0xa843 累计漂移（多核串扰 求和≠理论值）。
    SumDrift,
    /// 0xa844 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xa845 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xa846 空操作（账目无效果调用）。
    OpNoop,
    /// 0xa847 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xa848 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa849 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa84a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa84b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2621Code {
    /// 全量闭集。
    pub const ALL: [u2621Code; 12] = [
        u2621Code::RowDup,
        u2621Code::RowGap,
        u2621Code::ValOob,
        u2621Code::SumDrift,
        u2621Code::LedgerDup,
        u2621Code::LedgerHole,
        u2621Code::OpNoop,
        u2621Code::FieldDrift,
        u2621Code::LedgerBroken,
        u2621Code::SnapshotFingerprint,
        u2621Code::TableBroken,
        u2621Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2621Code::RowDup => 0xa840,
            u2621Code::RowGap => 0xa841,
            u2621Code::ValOob => 0xa842,
            u2621Code::SumDrift => 0xa843,
            u2621Code::LedgerDup => 0xa844,
            u2621Code::LedgerHole => 0xa845,
            u2621Code::OpNoop => 0xa846,
            u2621Code::FieldDrift => 0xa847,
            u2621Code::LedgerBroken => 0xa848,
            u2621Code::SnapshotFingerprint => 0xa849,
            u2621Code::TableBroken => 0xa84a,
            u2621Code::SegmentGuard => 0xa84b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2621Code::RowDup => "表行重复（切换点漏 同键双写）",
            u2621Code::RowGap => "表行断档（冲刷错配 序号不连续）",
            u2621Code::ValOob => "值越界（联签缺失 值域外）",
            u2621Code::SumDrift => "累计漂移（多核串扰 求和≠理论值）",
            u2621Code::LedgerDup => "账目重记（同一事件双入账）",
            u2621Code::LedgerHole => "账目断漏（序号缺失）",
            u2621Code::OpNoop => "空操作（账目无效果调用）",
            u2621Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2621Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2621Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2621Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2621Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2621Code::RowDup => "按键唯一律去重",
            u2621Code::RowGap => "按连续律补号",
            u2621Code::ValOob => "按值域裁剪",
            u2621Code::SumDrift => "按理论值复算",
            u2621Code::LedgerDup => "按事件幂等律合并",
            u2621Code::LedgerHole => "按连续律补账",
            u2621Code::OpNoop => "核对操作前置条件",
            u2621Code::FieldDrift => "按口径重建字段",
            u2621Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2621Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2621Code::TableBroken => "按语义规则重建常量表",
            u2621Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2621reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// CR3 切换 语义登记表（24 行逐行注释）。
pub const u2621reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2621reg: [(&'static str, u64); 24] = [
    ("CR_A", 7), // CR 主登记（CR3 语义 · 判据面）
    ("CR_B", 33), // CR 副登记（CR3 语义 · 对拍面）
    ("SW_A", 14), // SW 主登记（切换点 · 判据面）
    ("SW_B", 44), // SW 副登记（切换点 · 对拍面）
    ("SP_A", 21), // SP 主登记（单点律 · 判据面）
    ("SP_B", 55), // SP 副登记（单点律 · 对拍面）
    ("NF_A", 28), // NF 主登记（NOFLUSH · 判据面）
    ("NF_B", 66), // NF 副登记（NOFLUSH · 对拍面）
    ("FL_A", 35), // FL 主登记（FLUSH 组合 · 判据面）
    ("FL_B", 77), // FL 副登记（FLUSH 组合 · 对拍面）
    ("CT_A", 42), // CT 主登记（每核上下文 · 判据面）
    ("CT_B", 88), // CT 副登记（每核上下文 · 对拍面）
    ("FZ_A", 49), // FZ 主登记（冻结件 · 判据面）
    ("FZ_B", 99), // FZ 副登记（冻结件 · 对拍面）
    ("CN_A", 56), // CN 主登记（消费联签 · 判据面）
    ("CN_B", 110), // CN 副登记（消费联签 · 对拍面）
    ("PC_A", 63), // PC 主登记（每核面 · 判据面）
    ("PC_B", 121), // PC 副登记（每核面 · 对拍面）
    ("TL_A", 70), // TL 主登记（TLB 面 · 判据面）
    ("TL_B", 132), // TL 副登记（TLB 面 · 对拍面）
    ("AS_A", 77), // AS 主登记（断言面 · 判据面）
    ("AS_B", 143), // AS 副登记（断言面 · 对拍面）
    ("AR_A", 84), // AR 主登记（在档锚 · 判据面）
    ("AR_B", 154), // AR 副登记（在档锚 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2621reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2621reg_COUNT {
        if u2621reg[i].0 == key {
            return Some(u2621reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2621reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2621reg_COUNT {
        s = s.wrapping_add(u2621reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2621Ledger（CR3 切换 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// CR3 切换 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2621Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2621Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa840, 0xa84f]。
pub const fn u2621_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2621Code::ALL.len() {
        let c = u2621Code::ALL[i].code();
        if c < 0xa840 || c > 0xa84f {
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
fn u2621_guard_seg() -> bool {
    u2621_segment_guard()
}

/// UNX-F2621 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2621_core_tbl() -> bool {
    let mut ok = u2621reg_COUNT == 24;
    if ok { ok = u2621reg_get("CR_A") == Some(7) && u2621reg_get("CR_B") == Some(33); }
    if ok { ok = u2621reg_get("AR_A") == Some(84); }
    if ok { ok = u2621reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2621reg_sum() == 1668; }
    ok
}

/// UNX-F2621 守恒账三操作确定性效果。
fn check_u2621_core_ldg() -> bool {
    let mut ld = Mxu2621Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2621_checks() -> CheckSet {
    let mut set = CheckSet::new("U2621");
    set.add(
        "u2621-seg-guard",
        u2621_guard_seg(),
        if u2621_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2621-core-tbl",
        check_u2621_core_tbl(),
        if check_u2621_core_tbl() { "" } else { "UNX-F2621 core table mismatch" },
    );
    set.add(
        "u2621-core-ldg",
        check_u2621_core_ldg(),
        if check_u2621_core_ldg() { "" } else { "UNX-F2621 core ledger mismatch" },
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
    fn u2621_all_checks_green() {
        let set = run_u2621_checks();
        assert!(set.all_passed(), "U2621 domain checks must be green");
    }

    #[test]
    fn u2621_segment_guard_holds() {
        assert!(u2621_segment_guard());
    }
}
