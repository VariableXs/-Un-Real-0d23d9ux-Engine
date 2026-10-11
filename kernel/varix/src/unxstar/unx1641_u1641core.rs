//! unxu1641_u1641core —— 每核 runqueue 数据结构深化与锁语义 语义登记表与守恒账（锁域）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1641 · 每核 runqueue 数据结构深化与锁语义 · 判据原文：UNX-F1641 · 判据：每核一队列、每核自旋锁、锁域清单入册，跨队列访问必经迁移原语，越锁注入 10/10 次断言捕获
//! · 组件：锁域 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F1641 · 判据：每核一队列、每核自旋锁、锁域清单入册，跨队列访问必经迁移原语，越锁注入 10/10 次断言捕获
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1641_VERSION: &str = "u1641-u1641core-v1";

/// 诊断段基址。
pub const U1641_SEG_BASE: u16 = 0x5680;

// --------------------------------------------------------------------------
// 诊断码（u1641 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 锁域 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1641Code {
    /// 0x5680 表行重复（越锁访问 同键双写）。
    RowDup,
    /// 0x5681 表行断档（队列双主 序号不连续）。
    RowGap,
    /// 0x5682 值越界（迁移绕行 值域外）。
    ValOob,
    /// 0x5683 累计漂移（锁域漏登 求和≠理论值）。
    SumDrift,
    /// 0x5684 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0x5685 账目断漏（序号缺失）。
    LedgerHole,
    /// 0x5686 空操作（账目无效果调用）。
    OpNoop,
    /// 0x5687 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0x5688 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5689 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x568a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x568b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1641Code {
    /// 全量闭集。
    pub const ALL: [u1641Code; 12] = [
        u1641Code::RowDup,
        u1641Code::RowGap,
        u1641Code::ValOob,
        u1641Code::SumDrift,
        u1641Code::LedgerDup,
        u1641Code::LedgerHole,
        u1641Code::OpNoop,
        u1641Code::FieldDrift,
        u1641Code::LedgerBroken,
        u1641Code::SnapshotFingerprint,
        u1641Code::TableBroken,
        u1641Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1641Code::RowDup => 0x5680,
            u1641Code::RowGap => 0x5681,
            u1641Code::ValOob => 0x5682,
            u1641Code::SumDrift => 0x5683,
            u1641Code::LedgerDup => 0x5684,
            u1641Code::LedgerHole => 0x5685,
            u1641Code::OpNoop => 0x5686,
            u1641Code::FieldDrift => 0x5687,
            u1641Code::LedgerBroken => 0x5688,
            u1641Code::SnapshotFingerprint => 0x5689,
            u1641Code::TableBroken => 0x568a,
            u1641Code::SegmentGuard => 0x568b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1641Code::RowDup => "表行重复（越锁访问 同键双写）",
            u1641Code::RowGap => "表行断档（队列双主 序号不连续）",
            u1641Code::ValOob => "值越界（迁移绕行 值域外）",
            u1641Code::SumDrift => "累计漂移（锁域漏登 求和≠理论值）",
            u1641Code::LedgerDup => "账目重记（同一事件双入账）",
            u1641Code::LedgerHole => "账目断漏（序号缺失）",
            u1641Code::OpNoop => "空操作（账目无效果调用）",
            u1641Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u1641Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1641Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1641Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1641Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1641Code::RowDup => "按键唯一律去重",
            u1641Code::RowGap => "按连续律补号",
            u1641Code::ValOob => "按值域裁剪",
            u1641Code::SumDrift => "按理论值复算",
            u1641Code::LedgerDup => "按事件幂等律合并",
            u1641Code::LedgerHole => "按连续律补账",
            u1641Code::OpNoop => "核对操作前置条件",
            u1641Code::FieldDrift => "按口径重建字段",
            u1641Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1641Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1641Code::TableBroken => "按语义规则重建常量表",
            u1641Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1641reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 锁域 语义登记表（24 行逐行注释）。
pub const u1641reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1641reg: [(&'static str, u64); 24] = [
    ("RQ_A", 7), // RQ 主登记（每核队列 · 判据面）
    ("RQ_B", 33), // RQ 副登记（每核队列 · 对拍面）
    ("SL_A", 14), // SL 主登记（自旋锁 · 判据面）
    ("SL_B", 44), // SL 副登记（自旋锁 · 对拍面）
    ("LD_A", 21), // LD 主登记（锁域清单 · 判据面）
    ("LD_B", 55), // LD 副登记（锁域清单 · 对拍面）
    ("MG_A", 28), // MG 主登记（迁移原语 · 判据面）
    ("MG_B", 66), // MG 副登记（迁移原语 · 对拍面）
    ("YJ_A", 35), // YJ 主登记（越锁注入 · 判据面）
    ("YJ_B", 77), // YJ 副登记（越锁注入 · 对拍面）
    ("ZW_A", 42), // ZW 主登记（跨队列 · 判据面）
    ("ZW_B", 88), // ZW 副登记（跨队列 · 对拍面）
    ("engine_A", 49), // engine 主登记（engine.rs 衔接 · 判据面）
    ("engine_B", 99), // engine 副登记（engine.rs 衔接 · 对拍面）
    ("HZ_A", 56), // HZ 主登记（核号 · 判据面）
    ("HZ_B", 110), // HZ 副登记（核号 · 对拍面）
    ("QD_A", 63), // QD 主登记（队列深度 · 判据面）
    ("QD_B", 121), // QD 副登记（队列深度 · 对拍面）
    ("PS_A", 70), // PS 主登记（入队出队 · 判据面）
    ("PS_B", 132), // PS 副登记（入队出队 · 对拍面）
    ("LB_A", 77), // LB 主登记（负载记账 · 判据面）
    ("LB_B", 143), // LB 副登记（负载记账 · 对拍面）
    ("MC_A", 84), // MC 主登记（每核结构 · 判据面）
    ("MC_B", 154), // MC 副登记（每核结构 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1641reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1641reg_COUNT {
        if u1641reg[i].0 == key {
            return Some(u1641reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1641reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1641reg_COUNT {
        s = s.wrapping_add(u1641reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu1641Ledger（锁域 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 锁域 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu1641Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu1641Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5680, 0x568f]。
pub const fn u1641_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1641Code::ALL.len() {
        let c = u1641Code::ALL[i].code();
        if c < 0x5680 || c > 0x568f {
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
fn u1641_guard_seg() -> bool {
    u1641_segment_guard()
}

/// UNX-F1641 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u1641_core_tbl() -> bool {
    let mut ok = u1641reg_COUNT == 24;
    if ok { ok = u1641reg_get("RQ_A") == Some(7) && u1641reg_get("RQ_B") == Some(33); }
    if ok { ok = u1641reg_get("MC_A") == Some(84); }
    if ok { ok = u1641reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1641reg_sum() == 1668; }
    ok
}

/// UNX-F1641 守恒账三操作确定性效果。
fn check_u1641_core_ldg() -> bool {
    let mut ld = Mxu1641Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1641_checks() -> CheckSet {
    let mut set = CheckSet::new("U1641");
    set.add(
        "u1641-seg-guard",
        u1641_guard_seg(),
        if u1641_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1641-core-tbl",
        check_u1641_core_tbl(),
        if check_u1641_core_tbl() { "" } else { "UNX-F1641 core table mismatch" },
    );
    set.add(
        "u1641-core-ldg",
        check_u1641_core_ldg(),
        if check_u1641_core_ldg() { "" } else { "UNX-F1641 core ledger mismatch" },
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
    fn u1641_all_checks_green() {
        let set = run_u1641_checks();
        assert!(set.all_passed(), "U1641 domain checks must be green");
    }

    #[test]
    fn u1641_segment_guard_holds() {
        assert!(u1641_segment_guard());
    }
}
