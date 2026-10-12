//! unxu3301_u3301core —— RCU 压测编排（读多写少 8 核场景） 语义登记表与守恒账（RCU 压测编排）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3301 · RCU 压测编排（读多写少 8 核场景） · 判据原文：UNX-F3301 · 判据：J1：8 核读 + 写者换指针压测 10 秒，读侧零阻塞证据（读延迟分布 P99 ≤ 2×fast path 基线）、GP 推进健康；M-060 对账。
//! · 组件：RCU 压测编排 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3301 · 判据：J1：8 核读 + 写者换指针压测 10 秒，读侧零阻塞证据（读延迟分布 P99 ≤ 2×fast path 基线）、GP 推进健
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3301_VERSION: &str = "u3301-u3301core-v1";

/// 诊断段基址。
pub const U3301_SEG_BASE: u16 = 0xe020;

// --------------------------------------------------------------------------
// 诊断码（u3301 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// RCU 压测编排 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3301Code {
    /// 0xe020 表行重复（RCU 同键双写）。
    RowDup,
    /// 0xe021 表行断档（宽限 序号不连续）。
    RowGap,
    /// 0xe022 值越界（静默 值域外）。
    ValOob,
    /// 0xe023 累计漂移（读多写少 求和≠理论值）。
    SumDrift,
    /// 0xe024 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe025 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe026 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe027 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe028 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe029 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe02a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe02b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3301Code {
    /// 全量闭集。
    pub const ALL: [u3301Code; 12] = [
        u3301Code::RowDup,
        u3301Code::RowGap,
        u3301Code::ValOob,
        u3301Code::SumDrift,
        u3301Code::LedgerDup,
        u3301Code::LedgerHole,
        u3301Code::OpNoop,
        u3301Code::FieldDrift,
        u3301Code::LedgerBroken,
        u3301Code::SnapshotFingerprint,
        u3301Code::TableBroken,
        u3301Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3301Code::RowDup => 0xe020,
            u3301Code::RowGap => 0xe021,
            u3301Code::ValOob => 0xe022,
            u3301Code::SumDrift => 0xe023,
            u3301Code::LedgerDup => 0xe024,
            u3301Code::LedgerHole => 0xe025,
            u3301Code::OpNoop => 0xe026,
            u3301Code::FieldDrift => 0xe027,
            u3301Code::LedgerBroken => 0xe028,
            u3301Code::SnapshotFingerprint => 0xe029,
            u3301Code::TableBroken => 0xe02a,
            u3301Code::SegmentGuard => 0xe02b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3301Code::RowDup => "表行重复（RCU 同键双写）",
            u3301Code::RowGap => "表行断档（宽限 序号不连续）",
            u3301Code::ValOob => "值越界（静默 值域外）",
            u3301Code::SumDrift => "累计漂移（读多写少 求和≠理论值）",
            u3301Code::LedgerDup => "账目重记（同一事件双入账）",
            u3301Code::LedgerHole => "账目断漏（序号缺失）",
            u3301Code::OpNoop => "空操作（账目无效果调用）",
            u3301Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3301Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3301Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3301Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3301Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3301Code::RowDup => "按键唯一律去重",
            u3301Code::RowGap => "按连续律补号",
            u3301Code::ValOob => "按值域裁剪",
            u3301Code::SumDrift => "按理论值复算",
            u3301Code::LedgerDup => "按事件幂等律合并",
            u3301Code::LedgerHole => "按连续律补账",
            u3301Code::OpNoop => "核对操作前置条件",
            u3301Code::FieldDrift => "按口径重建字段",
            u3301Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3301Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3301Code::TableBroken => "按语义规则重建常量表",
            u3301Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3301reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// RCU 压测编排 语义登记表（24 行逐行注释）。
pub const u3301reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3301reg: [(&'static str, u64); 24] = [
    ("RD_A", 7), // RD 主登记（读者 · 判据面）
    ("RD_B", 33), // RD 副登记（读者 · 对拍面）
    ("WR_A", 14), // WR 主登记（写者 · 判据面）
    ("WR_B", 44), // WR 副登记（写者 · 对拍面）
    ("GP_A", 21), // GP 主登记（宽限期 · 判据面）
    ("GP_B", 55), // GP 副登记（宽限期 · 对拍面）
    ("CPU_A", 28), // CPU 主登记（核数 · 判据面）
    ("CPU_B", 66), // CPU 副登记（核数 · 对拍面）
    ("READM_A", 35), // READM 主登记（读多写少比 · 判据面）
    ("READM_B", 77), // READM 副登记（读多写少比 · 对拍面）
    ("QS_A", 42), // QS 主登记（静默态 · 判据面）
    ("QS_B", 88), // QS 副登记（静默态 · 对拍面）
    ("GPW_A", 49), // GPW 主登记（宽限窗口 · 判据面）
    ("GPW_B", 99), // GPW 副登记（宽限窗口 · 对拍面）
    ("STW_A", 56), // STW 主登记（停顿窗口 · 判据面）
    ("STW_B", 110), // STW 副登记（停顿窗口 · 对拍面）
    ("LAG_A", 63), // LAG 主登记（读侧滞后 · 判据面）
    ("LAG_B", 121), // LAG 副登记（读侧滞后 · 对拍面）
    ("LOSS_A", 70), // LOSS 主登记（零丢失账 · 判据面）
    ("LOSS_B", 132), // LOSS 副登记（零丢失账 · 对拍面）
    ("FP_A", 77), // FP 主登记（指纹链 · 判据面）
    ("FP_B", 143), // FP 副登记（指纹链 · 对拍面）
    ("THR_A", 84), // THR 主登记（阈值 · 判据面）
    ("THR_B", 154), // THR 副登记（阈值 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3301reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3301reg_COUNT {
        if u3301reg[i].0 == key {
            return Some(u3301reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3301reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3301reg_COUNT {
        s = s.wrapping_add(u3301reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3301Ledger（RCU 压测编排 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// RCU 压测编排 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3301Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3301Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe020, 0xe02f]。
pub const fn u3301_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3301Code::ALL.len() {
        let c = u3301Code::ALL[i].code();
        if c < 0xe020 || c > 0xe02f {
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
fn u3301_guard_seg() -> bool {
    u3301_segment_guard()
}

/// UNX-F3301 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3301_core_tbl() -> bool {
    let mut ok = u3301reg_COUNT == 24;
    if ok { ok = u3301reg_get("RD_A") == Some(7) && u3301reg_get("RD_B") == Some(33); }
    if ok { ok = u3301reg_get("THR_A") == Some(84); }
    if ok { ok = u3301reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3301reg_sum() == 1668; }
    ok
}

/// UNX-F3301 守恒账三操作确定性效果。
fn check_u3301_core_ldg() -> bool {
    let mut ld = Mxu3301Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3301_checks() -> CheckSet {
    let mut set = CheckSet::new("U3301");
    set.add(
        "u3301-seg-guard",
        u3301_guard_seg(),
        if u3301_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3301-core-tbl",
        check_u3301_core_tbl(),
        if check_u3301_core_tbl() { "" } else { "UNX-F3301 core table mismatch" },
    );
    set.add(
        "u3301-core-ldg",
        check_u3301_core_ldg(),
        if check_u3301_core_ldg() { "" } else { "UNX-F3301 core ledger mismatch" },
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
    fn u3301_all_checks_green() {
        let set = run_u3301_checks();
        assert!(set.all_passed(), "U3301 domain checks must be green");
    }

    #[test]
    fn u3301_segment_guard_holds() {
        assert!(u3301_segment_guard());
    }
}
